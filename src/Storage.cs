using System;
using System.IO;
using System.Text;
using System.Web.Script.Serialization;

namespace QPlay
{
    internal static class Json
    {
        internal static readonly JavaScriptSerializer Serializer = new JavaScriptSerializer { MaxJsonLength = 16 * 1024 * 1024 };

        internal static void Write(object value)
        {
            using (var output = new StreamWriter(Console.OpenStandardOutput(), new UTF8Encoding(false)))
                output.WriteLine(Serializer.Serialize(value));
        }

        internal static T Read<T>(string path)
        {
            return Serializer.Deserialize<T>(File.ReadAllText(path));
        }

        internal static void Save(string path, object value)
        {
            var temporary = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
            try
            {
                File.WriteAllText(temporary, Serializer.Serialize(value) + Environment.NewLine);
                if (File.Exists(path))
                    File.Replace(temporary, path, null);
                else
                    File.Move(temporary, path);
            }
            finally
            {
                if (File.Exists(temporary))
                    File.Delete(temporary);
            }
        }
    }

    internal sealed class PlayerPaths
    {
        internal readonly string Configuration;
        internal readonly string Data;
        internal readonly bool Portable;

        internal PlayerPaths(string profile)
        {
            Portable = File.Exists(Path.Combine(Program.InstallDirectory, "qplay.portable"));
            if (!String.IsNullOrEmpty(profile))
            {
                var root = Path.GetFullPath(profile);
                Configuration = Path.Combine(root, "config");
                Data = Path.Combine(root, "data");
            }
            else if (Portable)
            {
                Configuration = Path.Combine(Program.InstallDirectory, "userdata", "config");
                Data = Path.Combine(Program.InstallDirectory, "userdata", "data");
            }
            else
            {
                Configuration = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "qplay");
                Data = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "qplay");
            }
        }

        internal string SettingsFile { get { return Path.Combine(Configuration, "settings.json"); } }

        internal void Create()
        {
            Directory.CreateDirectory(Configuration);
            Directory.CreateDirectory(Data);
            Directory.CreateDirectory(Path.Combine(Data, "watch-later"));
            Directory.CreateDirectory(Path.Combine(Data, "framegen"));
        }

        internal Preferences Preferences()
        {
            return File.Exists(SettingsFile) ? Json.Read<Preferences>(SettingsFile) : new Preferences();
        }
    }

    internal sealed class Preferences
    {
        public string language { get; set; }
        public bool resume { get; set; }
        public bool frameGeneration { get; set; }

        public Preferences()
        {
            language = Ui.Language;
            resume = true;
            frameGeneration = true;
        }
    }
}
