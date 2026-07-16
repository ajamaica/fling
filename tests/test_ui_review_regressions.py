import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]


class UiReviewRegressionTest(unittest.TestCase):
    def test_install_timeout_covers_trainer_and_runtime_download_budgets(self):
        source = (ROOT / "ui/scripts/FlingClient.cs").read_text()
        self.assertIn("InstallTimeout = TimeSpan.FromMinutes(10)", source)
        self.assertIn("catch (OperationCanceledException)", source)
        self.assertIn("process.Kill(true)", source)
        self.assertIn("WaitForExitAsync(CancellationToken.None)", source)
        self.assertIn("if (ct.IsCancellationRequested) throw", source)

    def test_library_scroll_viewport_reserves_footer_clearance(self):
        source = (ROOT / "ui/scripts/Main.cs").read_text()

        build_library = re.search(
            r"private void BuildLibrary\(\)(.*?)\n    }\n\n    private async Task LoadGamesAsync",
            source,
            re.DOTALL,
        ).group(1)
        self.assertRegex(
            build_library,
            r"viewportSlot = new Control\s*\{(?s:.*?)"
            r"SizeFlagsVertical = SizeFlags\.ExpandFill(?s:.*?)ClipContents = true",
        )
        self.assertIn("_root.AddChild(viewportSlot)", build_library)
        self.assertIn("viewportSlot.AddChild(viewportMargins)", build_library)
        self.assertIn(
            "viewportMargins.SetAnchorsAndOffsetsPreset(LayoutPreset.FullRect)",
            build_library,
        )
        self.assertRegex(
            build_library,
            r'viewportMargins\.AddThemeConstantOverride\("margin_bottom",\s*\d+\)',
        )
        self.assertIn("viewportMargins.AddChild(scroll)", build_library)
        self.assertLess(
            build_library.index("_root.AddChild(viewportSlot)"),
            build_library.index("_root.AddChild(footer)"),
        )

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

    def test_library_card_text_stays_inside_symmetric_horizontal_insets(self):
        source = (ROOT / "ui/scripts/Main.cs").read_text()

        render_cards = re.search(
            r"private void RenderCards\(\)(.*?)\n    }\n\n    private async Task SetCardArtworkHintAsync",
            source,
            re.DOTALL,
        ).group(1)
        self.assertIn('inset.AddThemeConstantOverride("margin_left", 12)', render_cards)
        self.assertIn('inset.AddThemeConstantOverride("margin_right", 12)', render_cards)
        self.assertRegex(
            render_cards,
            r"content = new VBoxContainer\s*\{(?s:.*?)"
            r"SizeFlagsHorizontal = SizeFlags\.ExpandFill",
        )

        labels = re.findall(
            r"var (?:title|metadata) = new Label\s*\{(.*?)\n\s*\};",
            render_cards,
            re.DOTALL,
        )
        self.assertEqual(2, len(labels))
        for label in labels:
            self.assertIn("SizeFlagsHorizontal = SizeFlags.ExpandFill", label)
            self.assertIn("ClipText = true", label)
            self.assertIn(
                "TextOverrunBehavior = TextServer.OverrunBehavior.TrimEllipsis",
                label,
            )


if __name__ == "__main__":
    unittest.main(verbosity=2)
