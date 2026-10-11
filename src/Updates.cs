using System;
using System.Collections.Generic;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Security.Cryptography;
using System.Threading.Tasks;

namespace QPlay
{
    internal sealed class UpdateRelease
    {
        internal string Version;
        internal string InstallerUrl;
        internal string PortableUrl;
        internal string Digest;
        internal long Size;
    }

    internal sealed class UpdateCheckResult
    {
        internal string LatestVersion;
        internal UpdateRelease Update;
    }

    internal static class Updates
    {
        private const string ReleasePrefix = "https://github.com/qbutt-org/qplay/releases/download/";

        private static HttpClient Client()
        {
            ServicePointManager.SecurityProtocol = SecurityProtocolType.Tls12;
            var client = new HttpClient { Timeout = TimeSpan.FromMinutes(10) };
            client.DefaultRequestHeaders.UserAgent.ParseAdd("qplay/" + Program.Version);
            return client;
        }

        internal static async Task<UpdateCheckResult> Check()
        {
            using (var client = Client())
            using (var response = await client.GetAsync("https://api.github.com/repos/qbutt-org/qplay/releases/latest"))
            {
                if (response.StatusCode == HttpStatusCode.NotFound)
                    return new UpdateCheckResult();
                response.EnsureSuccessStatusCode();
                var json = await response.Content.ReadAsStringAsync();
                var data = Json.Serializer.Deserialize<Dictionary<string, object>>(json);
                var tag = Convert.ToString(data["tag_name"]);
                Version version;
                if (!tag.StartsWith("v", StringComparison.Ordinal) || !Version.TryParse(tag.Substring(1), out version))
                    throw new InvalidDataException(Ui.Text("В выпуске qplay указан неверный номер версии.",
                        "The qplay release contains an invalid version number."));
                var check = new UpdateCheckResult { LatestVersion = tag.Substring(1) };
                if (version <= new Version(Program.Version))
                    return check;
                var release = new UpdateRelease { Version = tag.Substring(1) };
                foreach (Dictionary<string, object> asset in (System.Collections.IEnumerable)data["assets"])
                {
                    var name = Convert.ToString(asset["name"]);
                    var url = Convert.ToString(asset["browser_download_url"]);
                    if (!url.StartsWith(ReleasePrefix + tag + "/", StringComparison.Ordinal))
                        throw new InvalidDataException(Ui.Text("В выпуске qplay указана неверная ссылка загрузки.",
                            "The qplay release contains an invalid download URL."));
                    if (name == "qplay-setup.exe")
                    {
                        release.InstallerUrl = url;
                        release.Size = Convert.ToInt64(asset["size"]);
                        release.Digest = Convert.ToString(asset["digest"]);
                    }
                    if (name == "qplay-portable.zip")
                        release.PortableUrl = url;
                }
                if (String.IsNullOrEmpty(release.InstallerUrl) || String.IsNullOrEmpty(release.PortableUrl)
                    || release.Size <= 0 || release.Size > 1024L * 1024 * 1024
                    || release.Digest == null || !System.Text.RegularExpressions.Regex.IsMatch(release.Digest, "^sha256:[a-f0-9]{64}$"))
                    throw new InvalidDataException(Ui.Text("В выпуске qplay отсутствуют проверенные файлы для Windows.",
                        "The qplay release is missing verified Windows assets."));
                check.Update = release;
                return check;
            }
        }

        internal static async Task<string> Download(UpdateRelease release, PlayerPaths paths, Action<int> progress)
        {
            var directory = Path.Combine(paths.Data, "updates");
            Directory.CreateDirectory(directory);
            var target = Path.Combine(directory, "qplay-" + release.Version + "-windows-x64-setup.exe");
            var partial = target + ".partial";
            try
            {
                using (var client = Client())
                using (var response = await client.GetAsync(release.InstallerUrl, HttpCompletionOption.ResponseHeadersRead))
                {
                    response.EnsureSuccessStatusCode();
                    using (var input = await response.Content.ReadAsStreamAsync())
                    using (var output = File.Create(partial))
                    using (var algorithm = SHA256.Create())
                    {
                        var buffer = new byte[128 * 1024];
                        var received = 0L;
                        var lastProgress = -1;
                        int length;
                        while ((length = await input.ReadAsync(buffer, 0, buffer.Length)) != 0)
                        {
                            received += length;
                            if (received > release.Size)
                                throw new InvalidDataException(Ui.Text("Размер обновления qplay превышает опубликованный.",
                                    "The qplay update exceeds its published size."));
                            algorithm.TransformBlock(buffer, 0, length, null, 0);
                            await output.WriteAsync(buffer, 0, length);
                            var percent = (int)(received * 100 / release.Size);
                            if (percent != lastProgress)
                            {
                                lastProgress = percent;
                                progress(percent);
                            }
                        }
                        algorithm.TransformFinalBlock(new byte[0], 0, 0);
                        var digest = "sha256:" + BitConverter.ToString(algorithm.Hash).Replace("-", "").ToLowerInvariant();
                        if (received != release.Size || digest != release.Digest)
                            throw new InvalidDataException(Ui.Text("Проверка целостности обновления qplay не пройдена.",
                                "The qplay update failed its integrity check."));
                    }
                }
                if (File.Exists(target))
                    File.Replace(partial, target, null);
                else
                    File.Move(partial, target);
                return target;
            }
            finally
            {
                if (File.Exists(partial))
                    File.Delete(partial);
            }
        }
    }
}
