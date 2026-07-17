using System;
using System.IO;
using System.Text.Json;
using FlingUi.Models;
using FlingUi.Scripts;

static void Assert(bool condition, string message)
{
    if (!condition) throw new InvalidOperationException(message);
}

const string gamesJson = """{"schema_version":1,"games":[{"appid":1245620,"name":"ELDEN RING","install_dir":"ELDEN RING Game","library_path":"/games","trainer_installed":false,"trainer_path":null,"running":false,"trainer_launch_delay_seconds":90,"trainer_instructions":["Use Windowed mode before activating the trainer.","Launch without Easy Anti-Cheat (EAC) and stay offline."]}]}""";
var games = JsonSerializer.Deserialize<GameListResponse>(gamesJson)!;
Assert(games.SchemaVersion == 1, "schema_version did not deserialize");
Assert(games.Games.Count == 1 && games.Games[0].AppId == 1245620, "numeric appid did not deserialize");
Assert(!games.Games[0].TrainerInstalled, "trainer state did not deserialize");
Assert(games.Games[0].TrainerLaunchDelaySeconds == 90, "special launch delay did not deserialize");
var guidance = GameGuidance.For(games.Games[0]);
Assert(guidance is not null, "Elden Ring guidance was missing");
Assert(guidance!.Contains("90 seconds"), "guidance omitted the launch delay");
Assert(guidance.Contains("Windowed mode"), "guidance omitted Windowed mode");
Assert(guidance.Contains("without Easy Anti-Cheat (EAC)"), "guidance omitted the EAC requirement");

const string commandJson = """{"schema_version":1,"success":false,"operation":"install","appid":20,"error_code":"network_error","message":"Trainer download failed","restart_required":false}""";
var command = JsonSerializer.Deserialize<CommandResponse>(commandJson)!;
Assert(!command.Success && command.ErrorCode == "network_error", "command error contract did not deserialize");
Assert(command.AppId == 20, "command appid was not numeric");

var namedCard = GameCardPresentation.For(games.Games[0]);
Assert(namedCard.Title == "ELDEN RING", "card title was not preserved");
Assert(namedCard.ArtworkFallback == "◆", "card did not provide a deliberate artwork fallback");

var unnamedGame = games.Games[0] with { Name = "   " };
var unnamedCard = GameCardPresentation.For(unnamedGame);
Assert(unnamedCard.Title == "Unknown game (AppID 1245620)", "blank game name did not get a visible title fallback");
Assert(unnamedCard.AccessibleText.Contains(unnamedCard.Title), "card accessible text omitted the fallback title");

var artworkRoot = Path.Combine(Path.GetTempPath(), $"fling-artwork-tests-{Guid.NewGuid():N}");
try
{
    var directCache = Path.Combine(artworkRoot, "10");
    Directory.CreateDirectory(directCache);
    var directHeader = Path.Combine(directCache, "library_header.jpg");
    File.WriteAllText(directHeader, "direct");
    Assert(LocalArtworkLocator.FindHeader(artworkRoot, 10) == directHeader, "direct library header was not found");

    var nestedCache = Path.Combine(artworkRoot, "20", "a1b2c3");
    Directory.CreateDirectory(nestedCache);
    var nestedHeader = Path.Combine(nestedCache, "library_header.jpg");
    File.WriteAllText(nestedHeader, "nested");
    Assert(LocalArtworkLocator.FindHeader(artworkRoot, 20) == nestedHeader, "nested library header was not found");

    Directory.CreateDirectory(Path.Combine(artworkRoot, "30"));
    Assert(LocalArtworkLocator.FindHeader(artworkRoot, 30) is null, "missing library header did not remain a miss");

    var outside = Path.Combine(artworkRoot, "outside");
    Directory.CreateDirectory(outside);
    File.WriteAllText(Path.Combine(outside, "library_header.jpg"), "outside");
    Directory.CreateSymbolicLink(Path.Combine(artworkRoot, "40"), outside);
    Assert(LocalArtworkLocator.FindHeader(artworkRoot, 40) is null, "header lookup escaped through an AppID symlink");

    var linkedCache = Path.Combine(artworkRoot, "50");
    Directory.CreateDirectory(linkedCache);
    Directory.CreateSymbolicLink(Path.Combine(linkedCache, "linked"), outside);
    Assert(LocalArtworkLocator.FindHeader(artworkRoot, 50) is null, "header lookup escaped through a nested symlink");
}
finally
{
    Directory.Delete(artworkRoot, recursive: true);
}
Console.WriteLine("Model JSON contract tests passed.");
