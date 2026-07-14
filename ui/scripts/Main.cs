using System.Text.Json;
using FlingUi.Models;
using Godot;

namespace FlingUi.Scripts;

public partial class Main : Control
{
    private readonly AppLogger _log = new();
    private FlingClient _client = null!;
    private readonly ArtworkService _artwork = new();
    private readonly List<SteamGame> _games = [];
    private VBoxContainer _root = null!;
    private GridContainer _grid = null!;
    private Label _status = null!;
    private LineEdit _search = null!;
    private Button _filterButton = null!;
    private CancellationTokenSource? _loadCts;
    private TrainerOperationState _operation = TrainerOperationState.Idle;
    private int _filter;
    private int? _restoreFocusAppId;
    private SteamGame? _detailsGame;
    private Label? _detailsPhase;
    private static readonly string[] Filters = ["All", "Installed", "Not installed"];

    public override void _Ready()
    {
        _client = new FlingClient(_log);
        BuildLibrary();
        CallDeferred(MethodName.ShowFirstRunIfNeeded);
        _ = LoadGamesAsync();
    }

    public override void _UnhandledInput(InputEvent e)
    {
        if (e.IsActionPressed("fling_refresh")) { GetViewport().SetInputAsHandled(); _ = LoadGamesAsync(); }
        else if (e.IsActionPressed("fling_toggle_trainer") && _detailsGame is not null && _detailsPhase is not null)
        { GetViewport().SetInputAsHandled(); _ = ModifyTrainerAsync(_detailsGame, _detailsPhase); }
        else if (e.IsActionPressed("fling_open_settings")) { GetViewport().SetInputAsHandled(); _ = ShowSettingsAsync(); }
        else if (e.IsActionPressed("fling_filter_previous")) { GetViewport().SetInputAsHandled(); ChangeFilter(-1); }
        else if (e.IsActionPressed("fling_filter_next")) { GetViewport().SetInputAsHandled(); ChangeFilter(1); }
        else if (e.IsActionPressed("ui_cancel") && _root.Name != "Library") { GetViewport().SetInputAsHandled(); BuildLibrary(); RenderCards(); }
    }

    private void BuildLibrary()
    {
        _detailsGame = null; _detailsPhase = null;
        Clear();
        _root = new VBoxContainer { Name = "Library" };
        _root.SetAnchorsAndOffsetsPreset(LayoutPreset.FullRect, LayoutPresetMode.Minsize, 28);
        _root.AddThemeConstantOverride("separation", 18); AddChild(_root);
        var header = new HBoxContainer(); _root.AddChild(header);
        var title = new Label { Text = "FLING", SizeFlagsHorizontal = SizeFlags.ExpandFill };
        title.AddThemeFontSizeOverride("font_size", 38); header.AddChild(title);
        _status = new Label { Text = "Finding Steam games…", VerticalAlignment = VerticalAlignment.Center }; header.AddChild(_status);
        var settings = Button("Settings  [S]", () => _ = ShowSettingsAsync()); header.AddChild(settings);
        var tools = new HBoxContainer(); tools.AddThemeConstantOverride("separation", 12); _root.AddChild(tools);
        _search = new LineEdit { PlaceholderText = "Search your library", SizeFlagsHorizontal = SizeFlags.ExpandFill, ClearButtonEnabled = true };
        _search.TextChanged += _ => RenderCards(); tools.AddChild(_search);
        _filterButton = Button($"Filter: {Filters[_filter]}  [LB/RB]", () => ChangeFilter(1)); tools.AddChild(_filterButton);
        tools.AddChild(Button("Refresh  [R]", () => _ = LoadGamesAsync()));
        var viewportSlot = new Control
        {
            SizeFlagsVertical = SizeFlags.ExpandFill,
            ClipContents = true
        };
        _root.AddChild(viewportSlot);
        var viewportMargins = new MarginContainer();
        viewportMargins.SetAnchorsAndOffsetsPreset(LayoutPreset.FullRect);
        viewportMargins.AddThemeConstantOverride("margin_bottom", 12);
        viewportSlot.AddChild(viewportMargins);
        var scroll = new ScrollContainer
        {
            SizeFlagsVertical = SizeFlags.ExpandFill,
            HorizontalScrollMode = ScrollContainer.ScrollMode.Disabled,
            FollowFocus = true
        }; viewportMargins.AddChild(scroll);
        _grid = new GridContainer { Columns = 4, SizeFlagsHorizontal = SizeFlags.ExpandFill }; _grid.AddThemeConstantOverride("h_separation", 16); _grid.AddThemeConstantOverride("v_separation", 16); scroll.AddChild(_grid);
        var footer = new Label { Text = "A Select   B Back   X Install/Remove   LB/RB Filter   Y Refresh   Menu Settings", Modulate = new Color("aeb5bd") };
        _root.AddChild(footer);
    }

    private async Task LoadGamesAsync()
    {
        _loadCts?.Cancel(); _loadCts = new CancellationTokenSource();
        if (IsInstanceValid(_status)) _status.Text = "Refreshing…";
        try
        {
            var response = await _client.GetGamesAsync(_loadCts.Token);
            if (!IsInsideTree()) return;
            _games.Clear(); _games.AddRange(response.Games.OrderBy(g => GameCardPresentation.For(g).Title));
            if (IsInstanceValid(_status)) _status.Text = $"{_games.Count} games";
            RenderCards();
        }
        catch (Exception e) when (e is FlingClientException or OperationCanceledException)
        {
            if (e is OperationCanceledException) return;
            _log.Error($"Library refresh failed: {e.Message}");
            if (IsInstanceValid(_status)) _status.Text = e.Message;
        }
    }

    private void RenderCards()
    {
        if (!IsInstanceValid(_grid)) return;
        foreach (var child in _grid.GetChildren()) child.QueueFree();
        var query = _search.Text.Trim();
        var visible = _games.Where(g => (_filter == 0 || (_filter == 1) == g.TrainerInstalled)
            && (query.Length == 0 || GameCardPresentation.For(g).Title.Contains(query, StringComparison.OrdinalIgnoreCase))).ToList();
        if (visible.Count == 0)
        {
            var empty = new Label { Text = _games.Count == 0 ? "No Steam games found. Check Settings for CLI status." : "No games match this search and filter." };
            empty.AddThemeFontSizeOverride("font_size", 24); _grid.AddChild(empty); return;
        }
        Button? focus = null;
        foreach (var game in visible)
        {
            var presentation = GameCardPresentation.For(game);
            var card = new Button
            {
                CustomMinimumSize = new Vector2(270, 250),
                TooltipText = presentation.AccessibleText,
                ClipContents = true
            };
            var inset = new MarginContainer { MouseFilter = MouseFilterEnum.Ignore };
            inset.SetAnchorsAndOffsetsPreset(LayoutPreset.FullRect);
            inset.AddThemeConstantOverride("margin_left", 12);
            inset.AddThemeConstantOverride("margin_top", 12);
            inset.AddThemeConstantOverride("margin_right", 12);
            inset.AddThemeConstantOverride("margin_bottom", 12);
            var content = new VBoxContainer
            {
                SizeFlagsHorizontal = SizeFlags.ExpandFill,
                MouseFilter = MouseFilterEnum.Ignore
            };
            var artworkViewport = new Control
            {
                CustomMinimumSize = new Vector2(0, 148),
                SizeFlagsHorizontal = SizeFlags.ExpandFill,
                SizeFlagsVertical = SizeFlags.ShrinkBegin,
                ClipContents = true,
                MouseFilter = MouseFilterEnum.Ignore
            };
            var artwork = new TextureRect
            {
                Texture = ArtworkService.Fallback,
                ExpandMode = TextureRect.ExpandModeEnum.IgnoreSize,
                StretchMode = TextureRect.StretchModeEnum.KeepAspectCovered,
                MouseFilter = MouseFilterEnum.Ignore
            };
            artworkViewport.AddChild(artwork);
            artwork.SetAnchorsAndOffsetsPreset(LayoutPreset.FullRect);
            content.AddChild(artworkViewport);
            var title = new Label
            {
                Text = presentation.Title,
                SizeFlagsHorizontal = SizeFlags.ExpandFill,
                ClipText = true,
                TextOverrunBehavior = TextServer.OverrunBehavior.TrimEllipsis,
                HorizontalAlignment = HorizontalAlignment.Center,
                MouseFilter = MouseFilterEnum.Ignore
            };
            title.AddThemeFontSizeOverride("font_size", 22); content.AddChild(title);
            var metadata = new Label
            {
                Text = $"{presentation.Status}  ·  AppID {game.AppId}",
                SizeFlagsHorizontal = SizeFlags.ExpandFill,
                ClipText = true,
                TextOverrunBehavior = TextServer.OverrunBehavior.TrimEllipsis,
                HorizontalAlignment = HorizontalAlignment.Center,
                MouseFilter = MouseFilterEnum.Ignore
            };
            content.AddChild(metadata);
            inset.AddChild(content);
            card.AddChild(inset);
            card.Pressed += () => ShowDetails(game); _grid.AddChild(card);
            if (game.AppId == _restoreFocusAppId) focus = card;
            _ = SetCardArtworkHintAsync(artwork, game);
        }
        (focus ?? _grid.GetChildOrNull<Button>(0))?.CallDeferred(Control.MethodName.GrabFocus);
    }

    private async Task SetCardArtworkHintAsync(TextureRect artwork, SteamGame game)
    {
        try
        {
            var texture = await _artwork.FindAsync(game);
            if (IsInstanceValid(artwork) && texture is not null) artwork.Texture = texture;
        }
        catch (Exception e)
        {
            // The fallback is already visible; a later library refresh retries the lookup.
            _log.Error($"Artwork lookup failed for AppID {game.AppId}: {e.Message}");
        }
    }

    private void ShowDetails(SteamGame game)
    {
        _detailsGame = game;
        _restoreFocusAppId = game.AppId; Clear();
        _root = new VBoxContainer { Name = "Details" }; _root.SetAnchorsAndOffsetsPreset(LayoutPreset.FullRect, LayoutPresetMode.Minsize, 44); _root.AddThemeConstantOverride("separation", 22); AddChild(_root);
        var back = Button("‹ Back  [B]", () => { BuildLibrary(); RenderCards(); }); _root.AddChild(back);
        var title = new Label { Text = game.Name }; title.AddThemeFontSizeOverride("font_size", 42); _root.AddChild(title);
        _root.AddChild(new Label { Text = $"Steam AppID  {game.AppId}\nTrainer  {(game.TrainerInstalled ? "Installed" : "Not installed")}" });
        if (game.TrainerPath is not null) _root.AddChild(new Label { Text = $"Advanced: {game.TrainerPath}", Modulate = new Color("aeb5bd"), AutowrapMode = TextServer.AutowrapMode.WordSmart });
        var phase = new Label { Text = "Ready" }; _detailsPhase = phase; _root.AddChild(phase);
        var action = Button(game.TrainerInstalled ? "Remove trainer  [X]" : "Install trainer  [X]", () => _ = ModifyTrainerAsync(game, phase));
        action.Disabled = _operation is TrainerOperationState.Installing or TrainerOperationState.Removing; _root.AddChild(action);
        back.CallDeferred(Control.MethodName.GrabFocus);
    }

    private async Task ModifyTrainerAsync(SteamGame game, Label phase)
    {
        if (_operation is TrainerOperationState.Installing or TrainerOperationState.Removing) return;
        _operation = game.TrainerInstalled ? TrainerOperationState.Removing : TrainerOperationState.Installing;
        phase.Text = game.TrainerInstalled ? "Removing trainer…" : "Searching, downloading, and validating trainer…";
        try
        {
            var result = game.TrainerInstalled ? await _client.RemoveAsync(game.AppId) : await _client.InstallAsync(game.AppId);
            _operation = TrainerOperationState.Succeeded;
            if (IsInstanceValid(phase)) phase.Text = result.Message;
            await LoadGamesAsync();
            var updated = _games.FirstOrDefault(g => g.AppId == game.AppId);
            if (updated is not null) ShowDetails(updated);
        }
        catch (FlingClientException e)
        {
            _operation = TrainerOperationState.Failed;
            if (IsInstanceValid(phase)) phase.Text = $"{e.Message}\nRetry with the action button. Technical detail is available in the log.";
            _log.Error($"Trainer operation failed: {e.Message}; {e.TechnicalDetail}");
        }
    }

    private async Task ShowSettingsAsync()
    {
        Clear(); _root = new VBoxContainer { Name = "Settings" }; _root.SetAnchorsAndOffsetsPreset(LayoutPreset.FullRect, LayoutPresetMode.Minsize, 44); _root.AddThemeConstantOverride("separation", 14); AddChild(_root);
        var back = Button("‹ Back  [B]", () => { BuildLibrary(); RenderCards(); }); _root.AddChild(back);
        var title = new Label { Text = "System status" }; title.AddThemeFontSizeOverride("font_size", 38); _root.AddChild(title);
        var values = new Label { Text = "Checking Fling and Steam…" }; _root.AddChild(values);
        var refresh = Button("Refresh status", () => _ = ShowSettingsAsync()); _root.AddChild(refresh);
        var restart = Button("Restart Steam…", ConfirmRestartSteam); _root.AddChild(restart);
        _root.AddChild(Button("Open logs", OpenLogs)); back.CallDeferred(Control.MethodName.GrabFocus);
        try
        {
            var s = await _client.GetStatusAsync(); if (!IsInstanceValid(values)) return;
            values.Text = $"CLI installed: {Yes(s.CliInstalled)}\nWatcher installed: {Yes(s.WatcherInstalled)}\nWatcher active: {Yes(s.WatcherActive)}\nGlobal environment configured: {Yes(s.GlobalEnvironmentConfigured)}\nSteam environment active: {Yes(s.SteamEnvironmentActive)}\nSteam running: {Yes(s.SteamRunning)}\nSteam root: {s.SteamRoot}\nTrainers directory: {s.TrainersDirectory}";
        }
        catch (FlingClientException e) { if (IsInstanceValid(values)) values.Text = e.Message; }
    }

    private void ConfirmRestartSteam()
    {
        var dialog = new ConfirmationDialog { Title = "Restart Steam?", DialogText = "Close any running games first. Steam may show a black screen briefly while restarting.", Exclusive = true };
        AddChild(dialog); dialog.Confirmed += async () => { try { await _client.RestartSteamAsync(); } catch (FlingClientException e) { _log.Error(e.Message); } };
        dialog.PopupCentered(); dialog.GetOkButton().GrabFocus();
    }

    private void OpenLogs()
    {
        var err = OS.ShellOpen("file://" + _log.Path); if (err != Error.Ok) _status.Text = $"Logs: {_log.Path}";
    }

    private void ChangeFilter(int delta) { _filter = (_filter + delta + Filters.Length) % Filters.Length; _filterButton.Text = $"Filter: {Filters[_filter]}  [LB/RB]"; RenderCards(); }
    private static string Yes(bool value) => value ? "Yes" : "No";
    private Button Button(string text, Action action) { var b = new Button { Text = text, FocusMode = FocusModeEnum.All }; b.Pressed += action; return b; }
    private void Clear() { foreach (var child in GetChildren()) child.QueueFree(); }

    private void ShowFirstRunIfNeeded()
    {
        var path = ProjectSettings.GlobalizePath("user://settings.json");
        try { if (File.Exists(path) && JsonDocument.Parse(File.ReadAllText(path)).RootElement.TryGetProperty("safety_acknowledged", out var value) && value.GetBoolean()) return; } catch { }
        var dialog = new AcceptDialog
        {
            Title = "Before you continue",
            Exclusive = true,
            DialogText = "Single-player only. Never use trainers in online or multiplayer games. Online services and anti-cheat systems may ban accounts or block the game. Trainers are third-party Windows executables and may be unsafe; only continue if you understand and accept this risk.",
            OkButtonText = "I understand and accept"
        };
        AddChild(dialog); dialog.Confirmed += () => { Directory.CreateDirectory(Path.GetDirectoryName(path)!); File.WriteAllText(path, "{\n  \"safety_acknowledged\": true\n}\n"); };
        dialog.PopupCentered(new Vector2I(760, 320)); dialog.GetOkButton().GrabFocus();
    }
}
