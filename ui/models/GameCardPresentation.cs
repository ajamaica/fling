namespace FlingUi.Models;

public sealed record GameCardPresentation(string Title, string Status, string ArtworkFallback, string AccessibleText)
{
    public static GameCardPresentation For(SteamGame game)
    {
        var title = string.IsNullOrWhiteSpace(game.Name) ? $"Unknown game (AppID {game.AppId})" : game.Name.Trim();
        var status = TrainerChoices.Normalize(game.TrainerChoice) switch
        {
            TrainerChoices.Wemod => game.WemodInstalled ? "WEMOD READY" : "WEMOD NOT INSTALLED",
            TrainerChoices.Both => TrainerChoices.IsReady(game) ? "FLING + WEMOD READY" : "FLING + WEMOD SETUP NEEDED",
            _ => game.TrainerInstalled ? "TRAINER READY" : "NOT INSTALLED"
        };
        return new GameCardPresentation(title, status, "◆", $"{title}. {status}. AppID {game.AppId}");
    }
}
