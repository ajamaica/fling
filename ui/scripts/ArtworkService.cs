using FlingUi.Models;
using Godot;

namespace FlingUi.Scripts;

public interface IArtworkProvider
{
    Task<Texture2D?> FindAsync(SteamGame game, CancellationToken cancellationToken = default);
}

public sealed class ArtworkService : IArtworkProvider
{
    private readonly Dictionary<int, Texture2D> _cache = new();
    public static Texture2D Fallback { get; } = CreateFallback();

    public Task<Texture2D?> FindAsync(SteamGame game, CancellationToken cancellationToken = default)
    {
        if (_cache.TryGetValue(game.AppId, out var cached)) return Task.FromResult<Texture2D?>(cached);
        cancellationToken.ThrowIfCancellationRequested();
        var home = System.Environment.GetFolderPath(System.Environment.SpecialFolder.UserProfile);
        var candidates = new[] {
            Path.Combine(home, ".local/share/Steam/appcache/librarycache", $"{game.AppId}_library_600x900.jpg"),
            Path.Combine(home, ".local/share/Steam/appcache/librarycache", game.AppId.ToString(), "library_600x900.jpg"),
            Path.Combine(game.LibraryPath, "appcache/librarycache", $"{game.AppId}_library_600x900.jpg")
        };
        foreach (var candidate in candidates)
        {
            try
            {
                if (!File.Exists(candidate)) continue;
                var image = Image.LoadFromFile(candidate);
                if (!image.IsEmpty()) return Task.FromResult<Texture2D?>(_cache[game.AppId] = ImageTexture.CreateFromImage(image));
            }
            catch { /* Artwork must never break the library. */ }
        }
        // Do not cache a miss: Steam may still be writing its cache during a refresh.
        return Task.FromResult<Texture2D?>(Fallback);
    }

    private static Texture2D CreateFallback()
    {
        var gradient = new Gradient { Colors = [new Color("26323a"), new Color("182026"), new Color("4b3519")] };
        return new GradientTexture2D
        {
            Gradient = gradient,
            Width = 256,
            Height = 144,
            FillFrom = new Vector2(0, 0),
            FillTo = new Vector2(1, 1)
        };
    }
}
