using System.Collections.Generic;

namespace FlingUi.Models;

/// <summary>Per-game choice of what starts with the game. FLiNG, WeMod, and both are equal options.</summary>
public static class TrainerChoices
{
    public const string Fling = "fling";
    public const string Wemod = "wemod";
    public const string Both = "both";

    public static IReadOnlyList<(string Value, string Label)> Options { get; } =
        [(Fling, "FLiNG"), (Wemod, "WeMod"), (Both, "Both")];

    public static string Normalize(string? choice) => choice is Wemod or Both ? choice : Fling;
    public static bool UsesFling(string? choice) => Normalize(choice) != Wemod;
    public static bool UsesWemod(string? choice) => Normalize(choice) != Fling;

    public static string Describe(string? choice) => Normalize(choice) switch
    {
        Wemod => "WeMod",
        Both => "FLiNG + WeMod",
        _ => "FLiNG"
    };

    /// <summary>True when everything the game is set to start is installed.</summary>
    public static bool IsReady(SteamGame game) => Normalize(game.TrainerChoice) switch
    {
        Wemod => game.WemodInstalled,
        Both => game.TrainerInstalled && game.WemodInstalled,
        _ => game.TrainerInstalled
    };
}
