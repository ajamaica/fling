using System;
using System.IO;

namespace FlingUi.Scripts;

public static class LocalArtworkLocator
{
    private const int MaxNestedDirectories = 256;

    public static string? FindHeader(string libraryCacheRoot, int appId)
    {
        try
        {
            var appCache = Path.GetFullPath(Path.Combine(libraryCacheRoot, appId.ToString()));
            if (!Directory.Exists(appCache) || IsReparsePoint(appCache)) return null;

            var direct = SafeHeader(appCache, appCache);
            if (direct is not null) return direct;

            var inspected = 0;
            foreach (var directory in Directory.EnumerateDirectories(appCache))
            {
                if (++inspected > MaxNestedDirectories) break;
                if (IsReparsePoint(directory)) continue;
                var nested = SafeHeader(appCache, directory);
                if (nested is not null) return nested;
            }
        }
        catch
        {
            // A partial or unreadable Steam cache is equivalent to missing artwork.
        }

        return null;
    }

    private static string? SafeHeader(string appCache, string directory)
    {
        var candidate = Path.GetFullPath(Path.Combine(directory, "library_header.jpg"));
        var appPrefix = appCache.EndsWith(Path.DirectorySeparatorChar)
            ? appCache
            : appCache + Path.DirectorySeparatorChar;
        if (!candidate.StartsWith(appPrefix, StringComparison.Ordinal) || !File.Exists(candidate)) return null;
        return IsReparsePoint(candidate) ? null : candidate;
    }

    private static bool IsReparsePoint(string path) =>
        (File.GetAttributes(path) & FileAttributes.ReparsePoint) != 0;
}
