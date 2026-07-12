using Godot;

namespace FlingUi.Scripts;

public sealed class AppLogger
{
    private const long MaxBytes = 512 * 1024;
    private readonly string _path = ProjectSettings.GlobalizePath("user://logs/fling-ui.log");
    public string Path => _path;

    public AppLogger()
    {
        Directory.CreateDirectory(System.IO.Path.GetDirectoryName(_path)!);
        if (File.Exists(_path) && new FileInfo(_path).Length > MaxBytes)
            File.Move(_path, _path + ".old", true);
    }

    public void Info(string message) => Write("INFO", message);
    public void Error(string message) => Write("ERROR", message);
    private void Write(string level, string message)
    {
        // Callers pass operation summaries only. Environment values and command output are excluded.
        var safe = message.Replace(System.Environment.GetFolderPath(System.Environment.SpecialFolder.UserProfile), "~");
        File.AppendAllText(_path, $"{DateTimeOffset.UtcNow:O} {level} {safe}\n");
    }
}
