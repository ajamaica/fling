using System.Collections.Generic;
using System.Linq;

namespace FlingUi.Models;

public static class GameGuidance
{
    public static string? For(SteamGame game)
    {
        var instructions = game.TrainerInstructions?
            .Where(instruction => !string.IsNullOrWhiteSpace(instruction))
            .Select(instruction => instruction.Trim())
            .ToList() ?? [];
        if (game.TrainerLaunchDelaySeconds <= 0 && instructions.Count == 0) return null;

        var lines = new List<string>();
        if (game.TrainerLaunchDelaySeconds > 0)
        {
            lines.Add($"Fling waits {game.TrainerLaunchDelaySeconds} seconds after the game is ready before launching the trainer.");
        }
        lines.AddRange(instructions);
        return "• " + string.Join("\n• ", lines);
    }
}
