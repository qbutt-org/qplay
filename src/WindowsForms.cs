using System;
using System.Drawing;
using System.IO;
using System.Windows.Forms;

namespace QPlay
{
    internal static class Ui
    {
        internal static string Language = System.Globalization.CultureInfo.CurrentUICulture.TwoLetterISOLanguageName == "ru" ? "ru" : "en";

        internal static string Text(string russian, string english)
        {
            return Language == "ru" ? russian : english;
        }

        internal static void Prepare(Form form, string title, Size size)
        {
            form.Text = title;
            form.ClientSize = size;
            form.MinimumSize = new Size(size.Width + 16, size.Height + 39);
            form.StartPosition = FormStartPosition.CenterScreen;
            form.Font = new Font("Segoe UI", 10);
            form.AutoScaleDimensions = new SizeF(96, 96);
            form.AutoScaleMode = AutoScaleMode.Dpi;
            form.AutoScroll = true;
            form.BackColor = Color.FromArgb(24, 27, 33);
            form.ForeColor = Color.FromArgb(232, 237, 242);
            form.Icon = new Icon(Path.Combine(Program.InstallDirectory, "qplay.ico"));
            form.Padding = new Padding(24);
            form.HandleCreated += delegate { Native.SetDarkTitleBar(form.Handle); };
        }

        internal static Button Button(string text)
        {
            return new Button {
                Text = text,
                AutoSize = true,
                Padding = new Padding(12, 6, 12, 6),
                FlatStyle = FlatStyle.Flat,
                BackColor = Color.FromArgb(0, 118, 190),
                ForeColor = Color.White,
                UseVisualStyleBackColor = false,
                Margin = new Padding(0, 8, 12, 8)
            };
        }

        internal static Label Label(string text)
        {
            return new Label { Text = text, AutoSize = true, Margin = new Padding(0, 4, 0, 8) };
        }
    }

    internal sealed class OpenForm : Form
    {
        internal string Media;
        internal OpenForm(PlayerPaths paths)
        {
            var russian = paths.Preferences().language == "ru";
            Ui.Prepare(this, "qplay", new Size(460, 300));
            MaximizeBox = false;
            var layout = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 1, RowCount = 5 };
            var title = Ui.Label("qplay");
            title.Font = new Font("Segoe UI Semibold", 24);
            layout.Controls.Add(title);
            var openFile = Ui.Button(russian ? "Открыть файл" : "Open file");
            openFile.Name = "openFile";
            openFile.Click += delegate {
                using (var chooser = new OpenFileDialog { Title = openFile.Text, CheckFileExists = true,
                    Filter = russian ? "Медиафайлы|*.mkv;*.mp4;*.webm;*.avi;*.mov;*.m4v;*.mp3;*.flac;*.ogg;*.wav|Все файлы|*.*"
                        : "Media files|*.mkv;*.mp4;*.webm;*.avi;*.mov;*.m4v;*.mp3;*.flac;*.ogg;*.wav|All files|*.*" })
                    if (chooser.ShowDialog(this) == DialogResult.OK)
                        Open(chooser.FileName);
            };
            layout.Controls.Add(openFile);
            var urlLabel = Ui.Label(russian ? "Ссылка на видео" : "Video URL");
            layout.Controls.Add(urlLabel);
            var urlRow = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 2, AutoSize = true };
            urlRow.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            urlRow.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
            var url = new TextBox { Name = "videoUrl", Dock = DockStyle.Fill, Margin = new Padding(0, 13, 12, 0),
                BackColor = Color.FromArgb(34, 38, 46), ForeColor = ForeColor, BorderStyle = BorderStyle.FixedSingle };
            var openUrl = Ui.Button(russian ? "Открыть" : "Open");
            openUrl.Name = "openUrl";
            openUrl.Click += delegate {
                Uri parsed;
                if (Uri.TryCreate(url.Text.Trim(), UriKind.Absolute, out parsed)
                    && (parsed.Scheme == "https" || parsed.Scheme == "http"))
                    Open(parsed.AbsoluteUri);
                else
                    MessageBox.Show(this, russian ? "Введите ссылку http или https." : "Enter an http or https URL.", "qplay");
            };
            urlRow.Controls.Add(url, 0, 0);
            urlRow.Controls.Add(openUrl, 1, 0);
            layout.Controls.Add(urlRow);
            var settings = new LinkLabel { Name = "settings", Text = russian ? "Настройки" : "Settings", AutoSize = true,
                LinkColor = Color.FromArgb(0, 157, 247), Margin = new Padding(0, 12, 0, 0) };
            settings.LinkClicked += delegate {
                using (var dialog = new SettingsForm(paths, null))
                    dialog.ShowDialog(this);
                russian = paths.Preferences().language == "ru";
                openFile.Text = russian ? "Открыть файл" : "Open file";
                urlLabel.Text = russian ? "Ссылка на видео" : "Video URL";
                openUrl.Text = russian ? "Открыть" : "Open";
                settings.Text = russian ? "Настройки" : "Settings";
            };
            layout.Controls.Add(settings);
            Controls.Add(layout);
            AcceptButton = openUrl;
            AllowDrop = true;
            DragEnter += delegate(object sender, DragEventArgs e) { if (e.Data.GetDataPresent(DataFormats.FileDrop)) e.Effect = DragDropEffects.Copy; };
            DragDrop += delegate(object sender, DragEventArgs e) {
                var files = e.Data.GetData(DataFormats.FileDrop) as string[];
                if (files != null && files.Length > 0)
                    Open(files[0]);
            };
        }

        private void Open(string media)
        {
            Media = media;
            DialogResult = DialogResult.OK;
            Close();
        }
    }
}
