using System.Collections.Generic;
using System.Text.Json.Serialization;

namespace FlingUi.Models;

public sealed record SteamGame(
    [property: JsonPropertyName("appid")] int AppId,
    [property: JsonPropertyName("name")] string Name,
    [property: JsonPropertyName("install_dir")] string InstallDir,
    [property: JsonPropertyName("library_path")] string LibraryPath,
    [property: JsonPropertyName("trainer_installed")] bool TrainerInstalled,
    [property: JsonPropertyName("trainer_path")] string? TrainerPath,
    [property: JsonPropertyName("running")] bool Running,
    [property: JsonPropertyName("trainer_launch_delay_seconds")] int TrainerLaunchDelaySeconds = 0,
    [property: JsonPropertyName("trainer_instructions")] IReadOnlyList<string>? TrainerInstructions = null);

public sealed record GameListResponse(
    [property: JsonPropertyName("schema_version")] int SchemaVersion,
    [property: JsonPropertyName("games")] List<SteamGame> Games);

public sealed record CommandResponse(
    [property: JsonPropertyName("schema_version")] int SchemaVersion,
    [property: JsonPropertyName("success")] bool Success,
    [property: JsonPropertyName("operation")] string Operation,
    [property: JsonPropertyName("appid")] int AppId,
    [property: JsonPropertyName("name")] string? Name,
    [property: JsonPropertyName("trainer_path")] string? TrainerPath,
    [property: JsonPropertyName("message")] string Message,
    [property: JsonPropertyName("error_code")] string? ErrorCode,
    [property: JsonPropertyName("restart_required")] bool RestartRequired);

public sealed record FlingStatus(
    [property: JsonPropertyName("schema_version")] int SchemaVersion,
    [property: JsonPropertyName("cli_installed")] bool CliInstalled,
    [property: JsonPropertyName("watcher_installed")] bool WatcherInstalled,
    [property: JsonPropertyName("watcher_active")] bool WatcherActive,
    [property: JsonPropertyName("global_environment_configured")] bool GlobalEnvironmentConfigured,
    [property: JsonPropertyName("steam_environment_active")] bool SteamEnvironmentActive,
    [property: JsonPropertyName("steam_running")] bool SteamRunning,
    [property: JsonPropertyName("steam_root")] string SteamRoot,
    [property: JsonPropertyName("trainers_directory")] string TrainersDirectory);

public enum TrainerOperationState { Idle, Installing, Removing, Succeeded, Failed }
