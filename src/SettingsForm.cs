using System;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Windows.Forms;

namespace QPlay
{
    internal sealed class SettingsForm : Form
    {
        private readonly PlayerPaths paths;
        private readonly string ipc;
        private readonly bool russian;
        private readonly Label updateStatus;
        private readonly Button checkUpdate;
        private readonly Button installUpdate;
        private UpdateRelease release;

        internal SettingsForm(PlayerPaths paths, string ipc)
        {
            this.paths = paths;
            this.ipc = ipc;
            var preferences = paths.Preferences();
            russian = preferences.language == "ru";
            Ui.Prepare(this, russian ? "qplay - Настройки" : "qplay - Settings", new Size(500, 386));
            MaximizeBox = false;
            var layout = new TableLayoutPanel { Dock = DockStyle.Top, ColumnCount = 1, AutoSize = true };
            var heading = Ui.Label("qplay " + Program.Version);
            heading.Font = new Font("Segoe UI Semibold", 18);
            heading.Margin = new Padding(0, 0, 0, 18);
            layout.Controls.Add(heading);

            var languageRow = new FlowLayoutPanel { AutoSize = true, Dock = DockStyle.Fill, WrapContents = false };
            languageRow.Controls.Add(Ui.Label(russian ? "Язык" : "Language"));
            var language = new ComboBox { Name = "language", DropDownStyle = ComboBoxStyle.DropDownList,
                Width = 180, Margin = new Padding(20, 0, 0, 12), FlatStyle = FlatStyle.Flat,
                BackColor = Color.FromArgb(34, 38, 46), ForeColor = ForeColor };
            language.Items.AddRange(new object[] { "Русский", "English" });
            language.SelectedIndex = russian ? 0 : 1;
            languageRow.Controls.Add(language);
            layout.Controls.Add(languageRow);
            var resume = new CheckBox { Name = "resume", Text = russian ? "Продолжать с места остановки" : "Resume where I stopped",
                AutoSize = true, Checked = preferences.resume, Margin = new Padding(0, 4, 0, 10) };
            var framegen = new CheckBox { Name = "frameGeneration", Text = russian ? "Генерация кадров в автоматическом режиме" : "Use frame generation in automatic mode",
                AutoSize = true, Checked = preferences.frameGeneration, Margin = new Padding(0, 4, 0, 16) };
            layout.Controls.Add(resume);
            layout.Controls.Add(framegen);

            updateStatus = Ui.Label("");
            updateStatus.Name = "updateStatus";
            updateStatus.Visible = false;
            updateStatus.MaximumSize = new Size(440, 0);
            updateStatus.Margin = new Padding(0, 12, 0, 0);
            layout.Controls.Add(updateStatus);
            var updateDetails = new ToolTip { AutoPopDelay = 15000 };
            Disposed += delegate { updateDetails.Dispose(); };
            var updates = new FlowLayoutPanel { AutoSize = true, Dock = DockStyle.Fill };
            checkUpdate = Ui.Button(russian ? "Проверить обновления" : "Check for updates");
            checkUpdate.Name = "checkUpdate";
            installUpdate = Ui.Button(russian ? "Обновить" : "Update");
            installUpdate.Name = "installUpdate";
            installUpdate.Visible = false;
            checkUpdate.Click += async delegate {
                checkUpdate.Enabled = false;
                release = null;
                installUpdate.Visible = false;
                updateDetails.SetToolTip(updateStatus, null);
                updateStatus.Visible = true;
                updateStatus.Text = russian ? "Проверка обновлений…" : "Checking for updates…";
                try
                {
                    var result = await Updates.Check();
                    if (IsDisposed)
                        return;
                    release = result.Update;
                    if (result.LatestVersion == null)
                        updateStatus.Text = russian ? "Пока нет опубликованной версии" : "No release is available yet";
                    else if (release == null)
                        updateStatus.Text = (russian ? "Установлена актуальная версия " : "You have the latest version ") + Program.Version;
                    else
                        updateStatus.Text = (russian ? "Доступна версия " : "Version available: ") + release.Version;
                    installUpdate.Visible = release != null;
                }
                catch (Exception error)
                {
                    if (!IsDisposed)
                    {
                        updateStatus.Text = russian ? "Не удалось проверить обновления" : "Couldn't check for updates";
                        updateDetails.SetToolTip(updateStatus, error.GetBaseException().Message);
                    }
                }
                finally
                {
                    if (!IsDisposed)
                        checkUpdate.Enabled = true;
                }
            };
            installUpdate.Click += async delegate {
                if (release == null)
                    return;
                installUpdate.Enabled = false;
                checkUpdate.Enabled = false;
                updateDetails.SetToolTip(updateStatus, null);
                try
                {
                    if (paths.Portable)
                    {
                        Process.Start(new ProcessStartInfo(release.PortableUrl) { UseShellExecute = true });
                        return;
                    }
                    var installer = await Updates.Download(release, paths, progress => {
                        if (!IsDisposed)
                            updateStatus.Text = (russian ? "Загрузка обновления: " : "Downloading update: ") + progress + "%";
                    });
                    if (IsDisposed)
                        return;
                    Native.StartInstaller(installer, "/CURRENTUSER /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /DIR="
                        + Native.Quote(Program.InstallDirectory) + " /QPLAYUPDATE=" + Process.GetCurrentProcess().Id);
                    try { PlayerIpc.Send(ipc, new object[] { "quit" }); }
                    catch (IOException) { }
                    catch (TimeoutException) { }
                    Close();
                }
                catch (Exception error)
                {
                    if (!IsDisposed)
                    {
                        updateStatus.Text = russian ? "Не удалось установить обновление" : "Couldn't install the update";
                        updateDetails.SetToolTip(updateStatus, error.GetBaseException().Message);
                    }
                }
                finally
                {
                    if (!IsDisposed)
                    {
                        installUpdate.Enabled = true;
                        checkUpdate.Enabled = true;
                    }
                }
            };
            updates.Controls.Add(checkUpdate);
            updates.Controls.Add(installUpdate);
            layout.Controls.Add(updates);

            var save = Ui.Button(russian ? "Сохранить" : "Save");
            save.Name = "saveSettings";
            save.Anchor = AnchorStyles.Right;
            save.Margin = new Padding(0, 16, 0, 0);
            save.Click += delegate {
                preferences = paths.UpdatePreferences(updated => {
                    updated.language = language.SelectedIndex == 0 ? "ru" : "en";
                    updated.resume = resume.Checked;
                    updated.frameGeneration = framegen.Checked;
                });
                Ui.Language = preferences.language;
                try
                {
                    PlayerIpc.Send(ipc,
                        new object[] { "set_property", "user-data/qplay-language", preferences.language },
                        new object[] { "set_property", "user-data/qplay-auto-framegen", preferences.frameGeneration },
                        new object[] { "set_property", "save-position-on-quit", preferences.resume },
                        new object[] { "set_property", "resume-playback", preferences.resume });
                }
                catch (IOException) { }
                catch (TimeoutException) { }
                Close();
            };
            layout.Controls.Add(save);
            Controls.Add(layout);
            AcceptButton = save;
        }
    }
}
