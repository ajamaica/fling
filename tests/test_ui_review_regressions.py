import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


class UiReviewRegressionTest(unittest.TestCase):
    def test_async_ui_updates_are_guarded_after_await(self):
        source = (ROOT / "ui/scripts/Main.cs").read_text()

        load_games = re.search(
            r"private async Task LoadGamesAsync\(\)(.*?)\n    }\n\n    private void RenderCards",
            source,
            re.DOTALL,
        ).group(1)
        self.assertRegex(
            load_games,
            r"await _client\.GetGamesAsync\([^;]+;\s*if \(!IsInsideTree\(\)\) return;",
        )
        self.assertRegex(load_games, r"if \(IsInstanceValid\(_status\)\) _status\.Text")
        self.assertNotRegex(load_games, r"!IsInstanceValid\(_status\)[^\n]*\) return;")

        modify_trainer = re.search(
            r"private async Task ModifyTrainerAsync\(SteamGame game, Label phase\)(.*?)\n    }\n\n    private async Task ShowSettingsAsync",
            source,
            re.DOTALL,
        ).group(1)
        self.assertRegex(
            modify_trainer,
            r"(?s)await _client\.(?:RemoveAsync|InstallAsync)[^;]+;.*?if \(IsInstanceValid\(phase\)\) phase\.Text",
        )
        self.assertRegex(
            modify_trainer,
            r"(?s)catch \(FlingClientException e\).*?if \(IsInstanceValid\(phase\)\) phase\.Text",
        )

    def test_artwork_fallback_is_shared_without_caching_misses(self):
        source = (ROOT / "ui/scripts/ArtworkService.cs").read_text()

        self.assertRegex(source, r"public static Texture2D Fallback { get; } = CreateFallback\(\);")
        self.assertRegex(source, r"return Task\.FromResult<Texture2D\?>\(Fallback\);")
        self.assertNotRegex(source, r"_cache\[game\.AppId\]\s*=\s*\w*[Ff]allback")

        main_source = (ROOT / "ui/scripts/Main.cs").read_text()
        self.assertIn("Texture = ArtworkService.Fallback", main_source)

    def test_library_cards_cover_the_bounded_artwork_viewport_without_inner_padding(self):
        source = (ROOT / "ui/scripts/Main.cs").read_text()

        render_cards = re.search(
            r"private void RenderCards\(\)(.*?)\n    }\n\n    private async Task SetCardArtworkHintAsync",
            source,
            re.DOTALL,
        ).group(1)
        self.assertRegex(render_cards, r"card = new Button\s*\{(?s:.*?)ClipContents = true")
        self.assertRegex(
            render_cards,
            r"artworkViewport = new (?:PanelContainer|Control)\s*\{(?s:.*?)"
            r"CustomMinimumSize = new Vector2\(0, 148\)(?s:.*?)ClipContents = true",
        )
        self.assertNotIn("artworkPadding", render_cards)
        self.assertIn("content.AddChild(artworkViewport)", render_cards)
        self.assertIn(
            "artwork.SetAnchorsAndOffsetsPreset(LayoutPreset.FullRect)", render_cards
        )
        self.assertIn("StretchMode = TextureRect.StretchModeEnum.KeepAspectCovered", render_cards)
        self.assertNotIn("StretchMode = TextureRect.StretchModeEnum.KeepAspectCentered", render_cards)
        self.assertLess(
            render_cards.index("content.AddChild(artworkViewport)"),
            render_cards.index("content.AddChild(title)"),
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
