using System;
using System.Text.Json;
using FlingUi.Models;

static void Assert(bool condition, string message)
{
    if (!condition) throw new InvalidOperationException(message);
}

const string gamesJson = """{"schema_version":1,"games":[{"appid":20,"name":"Space Game","install_dir":"Space Game","library_path":"/games","trainer_installed":false,"trainer_path":null,"running":false}]}""";
var games = JsonSerializer.Deserialize<GameListResponse>(gamesJson)!;
Assert(games.SchemaVersion == 1, "schema_version did not deserialize");
Assert(games.Games.Count == 1 && games.Games[0].AppId == 20, "numeric appid did not deserialize");
Assert(!games.Games[0].TrainerInstalled, "trainer state did not deserialize");

const string commandJson = """{"schema_version":1,"success":false,"operation":"install","appid":20,"error_code":"network_error","message":"Trainer download failed","restart_required":false}""";
var command = JsonSerializer.Deserialize<CommandResponse>(commandJson)!;
Assert(!command.Success && command.ErrorCode == "network_error", "command error contract did not deserialize");
Assert(command.AppId == 20, "command appid was not numeric");

var namedCard = GameCardPresentation.For(games.Games[0]);
Assert(namedCard.Title == "Space Game", "card title was not preserved");
Assert(namedCard.ArtworkFallback == "◆", "card did not provide a deliberate artwork fallback");

var unnamedGame = games.Games[0] with { Name = "   " };
var unnamedCard = GameCardPresentation.For(unnamedGame);
Assert(unnamedCard.Title == "Unknown game (AppID 20)", "blank game name did not get a visible title fallback");
Assert(unnamedCard.AccessibleText.Contains(unnamedCard.Title), "card accessible text omitted the fallback title");
Console.WriteLine("Model JSON contract tests passed.");
