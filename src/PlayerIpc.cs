using System;
using System.IO;
using System.IO.Pipes;
using System.Text;

namespace QPlay
{
    internal static class PlayerIpc
    {
        internal static void Send(string name, params object[][] commands)
        {
            if (String.IsNullOrEmpty(name))
                return;
            const string prefix = "\\\\.\\pipe\\";
            if (!name.StartsWith(prefix, StringComparison.OrdinalIgnoreCase))
                throw new ArgumentException(Ui.Text("Плеер qplay использует только локальное соединение управления.",
                    "qplay player IPC must be a local named pipe."));
            using (var pipe = new NamedPipeClientStream(".", name.Substring(prefix.Length), PipeDirection.Out))
            {
                pipe.Connect(1500);
                using (var output = new StreamWriter(pipe, new UTF8Encoding(false)))
                {
                    foreach (var command in commands)
                        output.WriteLine(Json.Serializer.Serialize(new { command = command }));
                }
            }
        }
    }
}
