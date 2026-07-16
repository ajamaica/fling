using System.Diagnostics;
using System.Text.Json;
using FlingUi.Models;

namespace FlingUi.Scripts;

public sealed class FlingClient
{
    public static readonly TimeSpan GamesTimeout = TimeSpan.FromSeconds(15);
    public static readonly TimeSpan StatusTimeout = TimeSpan.FromSeconds(10);
    // Trainer and optional runtime-support downloads each have a bounded
    // network window; this deadline covers both plus discovery and commit.
    public static readonly TimeSpan InstallTimeout = TimeSpan.FromMinutes(10);
    public static readonly TimeSpan RemoveTimeout = TimeSpan.FromSeconds(30);
    public static readonly TimeSpan RestartTimeout = TimeSpan.FromSeconds(60);
    private readonly AppLogger _log;
    private readonly string _path;
    private readonly bool _mock = Environment.GetEnvironmentVariable("FLING_UI_MOCK") == "1";
    private bool _mockInstalled;

    public FlingClient(AppLogger log)
    {
        _log = log;
        _path = Environment.GetEnvironmentVariable("FLING_CLI_PATH")
            ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".local", "bin", "fling");
        if (!File.Exists(_path) && File.Exists("../bin/fling")) _path = Path.GetFullPath("../bin/fling");
    }

    public Task<GameListResponse> GetGamesAsync(CancellationToken ct = default) => _mock
        ? Task.FromResult(MockGames()) : RunAsync<GameListResponse>(["games", "--json"], GamesTimeout, ct);
    public Task<FlingStatus> GetStatusAsync(CancellationToken ct = default) => _mock
        ? Task.FromResult(new FlingStatus(1, true, true, true, true, true, true, "~/.local/share/Steam", "~/Trainers"))
        : RunAsync<FlingStatus>(["status", "--json"], StatusTimeout, ct);
    public async Task<CommandResponse> InstallAsync(int appId, CancellationToken ct = default)
    {
        if (_mock) { await Task.Delay(550, ct); _mockInstalled = true; return MockCommand("install", appId, true); }
        return await RunAsync<CommandResponse>(["install", appId.ToString(), "--json"], InstallTimeout, ct);
    }
    public async Task<CommandResponse> RemoveAsync(int appId, CancellationToken ct = default)
    {
        if (_mock) { await Task.Delay(350, ct); _mockInstalled = false; return MockCommand("remove", appId, true); }
        return await RunAsync<CommandResponse>(["remove", appId.ToString(), "--json"], RemoveTimeout, ct);
    }
    public Task<CommandResponse> RefreshAsync(int appId, CancellationToken ct = default) => _mock
        ? Task.FromResult(MockCommand("refresh", appId, true))
        : RunAsync<CommandResponse>(["refresh", appId.ToString(), "--json"], GamesTimeout, ct);
    public async Task RestartSteamAsync(CancellationToken ct = default)
    {
        if (_mock) { await Task.Delay(500, ct); return; }
        await RunRawAsync(["restart-steam"], RestartTimeout, ct);
    }

    private async Task<T> RunAsync<T>(string[] args, TimeSpan timeout, CancellationToken ct)
    {
        var result = await RunRawAsync(args, timeout, ct);
        try { return JsonSerializer.Deserialize<T>(result.Stdout) ?? throw new FlingClientException("The CLI returned an empty response."); }
        catch (JsonException e) { throw new FlingClientException("The CLI returned an unreadable response.", e.Message); }
    }

    private async Task<(string Stdout, string Stderr)> RunRawAsync(string[] args, TimeSpan timeout, CancellationToken ct)
    {
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(ct); linked.CancelAfter(timeout);
        var psi = new ProcessStartInfo
        {
            FileName = _path,
            UseShellExecute = false,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            CreateNoWindow = true
        };
        foreach (var arg in args) psi.ArgumentList.Add(arg);
        _log.Info($"CLI operation {args[0]} started");
        using var process = new Process { StartInfo = psi };
        try
        {
            if (!process.Start()) throw new FlingClientException("Could not start the Fling CLI.");
            var stdout = process.StandardOutput.ReadToEndAsync(linked.Token);
            var stderr = process.StandardError.ReadToEndAsync(linked.Token);
            await process.WaitForExitAsync(linked.Token);
            var output = await stdout; var error = await stderr;
            if (process.ExitCode != 0) throw MapError(process.ExitCode, output, error);
            _log.Info($"CLI operation {args[0]} completed");
            return (output, error);
        }
        catch (OperationCanceledException)
        {
            try
            {
                if (!process.HasExited)
                {
                    process.Kill(true);
                    await process.WaitForExitAsync(CancellationToken.None);
                }
            }
            catch { }
            if (ct.IsCancellationRequested) throw;
            throw new FlingClientException("The operation timed out. Please try again.");
        }
        catch (System.ComponentModel.Win32Exception)
        { throw new FlingClientException("Fling CLI was not found. Open Settings for installation details."); }
    }

    private static FlingClientException MapError(int code, string stdout, string stderr)
    {
        string? message = null;
        try { message = JsonDocument.Parse(stdout).RootElement.GetProperty("message").GetString(); } catch { }
        message ??= code switch
        {
            2 => "Check the requested game and try again.",
            3 => "Steam game not found.",
            4 => "No remote trainer was found.",
            5 => "The download failed. Check your connection.",
            6 => "The downloaded trainer was invalid.",
            7 => "No local trainer is installed.",
            8 => "A required dependency is missing.",
            9 => "An unsafe trainer path was refused.",
            10 => "Steam configuration failed.",
            11 => "Required game runtime support could not be installed safely.",
            12 => "Managed game runtime files changed; removal was refused.",
            _ => "The Fling CLI operation failed."
        };
        return new FlingClientException(message, string.IsNullOrWhiteSpace(stderr) ? $"Exit code {code}" : $"Exit code {code}: {stderr.Trim()}");
    }

    private GameListResponse MockGames() => new(1,
    [
        new(367520, "Hollow Knight", "Hollow Knight", "~/.local/share/Steam", _mockInstalled, _mockInstalled ? "~/Trainers/367520 - Hollow Knight/Trainer.exe" : null, false),
        new(413150, "Stardew Valley", "Stardew Valley", "~/.local/share/Steam", true, "~/Trainers/413150 - Stardew Valley/Trainer.exe", true),
        new(620, "Portal 2", "Portal 2", "~/.local/share/Steam", false, null, false)
    ]);
    private static CommandResponse MockCommand(string op, int id, bool ok) => new(1, ok, op, id, "Mock game", null,
        op == "remove" ? "Trainer removed successfully" : op == "install" ? "Trainer installed successfully" : "Game state refreshed", null, false);
}

public sealed class FlingClientException(string message, string? technicalDetail = null) : Exception(message)
{
    public string? TechnicalDetail { get; } = technicalDetail;
}
