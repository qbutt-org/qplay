#include <Windows.h>
#include <TlHelp32.h>

#include <cmath>
#include <cstdio>
#include <memory>
#include <stdexcept>

#include <QDir>
#include <QElapsedTimer>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QImage>
#include <QJsonDocument>
#include <QJsonObject>
#include <QMetaObject>
#include <QMouseEvent>
#include <QOffscreenSurface>
#include <QOpenGLContext>
#include <QOpenGLFramebufferObject>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickGraphicsDevice>
#include <QQuickItem>
#include <QQuickRenderControl>
#include <QQuickRenderTarget>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QStringList>
#include <QThread>
#include <QThreadPool>

#include "qplay_native/src/controller.cxxqt.h"
#include "qplay_native/src/track_model.cxxqt.h"

namespace
{
void require(bool condition, const char *message)
{
    if (!condition)
        throw std::runtime_error(message);
}

int childProcessCount(bool report = true)
{
    const auto snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    require(snapshot != INVALID_HANDLE_VALUE, "Process snapshot failed");
    int count = 0;
    PROCESSENTRY32W process {sizeof(process)};
    if (Process32FirstW(snapshot, &process))
    {
        do
        {
            if (process.th32ParentProcessID == GetCurrentProcessId())
            {
                ++count;
                if (report)
                    std::fwprintf(stderr, L"Owned child: %lu %s\n", process.th32ProcessID, process.szExeFile);
            }
        }
        while (Process32NextW(snapshot, &process));
    }
    CloseHandle(snapshot);
    return count;
}

qint64 selectedSubtitle(Qplay::TrackModel *model)
{
    for (int row = 0; row < model->rowCount({}); ++row)
    {
        const auto index = model->index(row, 0, {});
        if (model->data(index, 257).toString() == QStringLiteral("sub")
            && model->data(index, 259).toBool())
        {
            return model->data(index, 256).toLongLong();
        }
    }
    return -1;
}

QQuickItem *findVisualItem(QQuickItem *parent, const QString &name)
{
    if (!parent)
        return nullptr;
    if (parent->objectName() == name)
        return parent;
    for (auto *child : parent->childItems())
    {
        if (auto *found = findVisualItem(child, name))
            return found;
    }
    return nullptr;
}

void click(QQuickWindow &window, QQuickItem &item)
{
    const auto point = item.mapToScene(QPointF(item.width() / 2, item.height() / 2));
    QMouseEvent press(QEvent::MouseButtonPress, point, point, point,
        Qt::LeftButton, Qt::LeftButton, Qt::NoModifier);
    QMouseEvent release(QEvent::MouseButtonRelease, point, point, point,
        Qt::LeftButton, Qt::NoButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&window, &press);
    QCoreApplication::sendEvent(&window, &release);
}

void hover(QQuickWindow &window, QQuickItem &item)
{
    const auto point = item.mapToScene(QPointF(item.width() / 2, item.height() / 2));
    QMouseEvent move(QEvent::MouseMove, point, point, point,
        Qt::NoButton, Qt::NoButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&window, &move);
}
}

int main(int argc, char *argv[])
{
    if (argc != 6)
        return 2;

    QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    QQuickStyle::setStyle(QStringLiteral("Basic"));
    QGuiApplication app(argc, argv);
    const QStringList arguments = app.arguments();
    const QString library = arguments.at(1);
    const QString media = arguments.at(2);
    const QString secondMedia = arguments.at(3);
    const QString profile = arguments.at(4);
    const QString evidencePrefix = arguments.at(5);
    if (qEnvironmentVariableIsEmpty("QPLAY_MEDIA_RUNTIME"))
        qputenv("QPLAY_MEDIA_RUNTIME", QDir(profile).absoluteFilePath(QStringLiteral("framegen")).toUtf8());
    QJsonObject evidence;
    int exitCode = 0;
    bool pendingCatalogCancellation = false;

    try
    {
        QOpenGLContext gl;
        require(gl.create(), "Create OpenGL context failed");
        QOffscreenSurface surface;
        surface.setFormat(gl.format());
        surface.create();
        require(gl.makeCurrent(&surface), "Make OpenGL context current failed");

        if (qEnvironmentVariableIsSet("QPLAY_CODEX_CATALOG_QA"))
        {
            {
                Qplay::PlayerController temporary;
                temporary.loadSubtitleModels();
                QElapsedTimer catalogStart;
                catalogStart.start();
                while (childProcessCount(false) == 0 && catalogStart.elapsed() < 5000)
                {
                    app.processEvents();
                    QThread::msleep(10);
                }
                require(childProcessCount(false) > 0,
                    "A pending Codex model catalog was not observed");
            }
            require(QThreadPool::globalInstance()->waitForDone(15000),
                "Abandoned Codex model catalog did not stop");
            require(childProcessCount(false) == 0,
                "Closing during model discovery left a child runtime");
            pendingCatalogCancellation = true;
        }

        {
            Qplay::PlayerController backend;
            backend.setDisplayRate(60);
            const QString preferences = QDir(profile).absoluteFilePath(QStringLiteral("config/preferences.json"));
            backend.configure(preferences, QStringLiteral("en_US"));
            backend.configureInstallation(profile, profile, true);
            backend.changeLanguage(QStringLiteral("ru"));
            backend.changeAppearance(QStringLiteral("light"));
            backend.changeAudioDelay(0.15);
            require(backend.settingsError().isEmpty(), "Saving player preferences failed");
            {
                Qplay::PlayerController restored;
                restored.configure(preferences, QStringLiteral("en_US"));
                require(restored.language() == QStringLiteral("ru")
                    && restored.appearance() == QStringLiteral("light")
                    && std::abs(restored.audioDelay() - 0.15) < 0.001,
                    "Player preferences did not persist");
            }
            backend.changeLanguage(QStringLiteral("en"));
            backend.changeAppearance(QStringLiteral("dark"));
            backend.changeFramegen(QStringLiteral("off"));
            QFile savedPreferences(preferences);
            require(savedPreferences.open(QIODevice::ReadOnly), "Read saved player preferences failed");
            auto settings = QJsonDocument::fromJson(savedPreferences.readAll()).object();
            savedPreferences.close();
            auto translation = settings.value(QStringLiteral("subtitle_translation")).toObject();
            translation.insert(QStringLiteral("model"), QStringLiteral("retired-model"));
            translation.insert(QStringLiteral("effort"), QStringLiteral("retired-effort"));
            settings.insert(QStringLiteral("subtitle_translation"), translation);
            require(savedPreferences.open(QIODevice::WriteOnly | QIODevice::Truncate)
                    && savedPreferences.write(QJsonDocument(settings).toJson()) > 0,
                "Save unavailable subtitle choices failed");
            savedPreferences.close();
            backend.configure(preferences, QStringLiteral("en_US"));
            require(backend.subtitleModel() == QStringLiteral("retired-model")
                && backend.subtitleEffort() == QStringLiteral("retired-effort"),
                "Unavailable subtitle choices were not restored");

            QQuickRenderControl control;
            QQuickWindow window(&control);
            window.setColor(Qt::black);
            window.setGeometry(0, 0, 1280, 720);
            window.setGraphicsDevice(QQuickGraphicsDevice::fromOpenGLContext(&gl));
            require(control.initialize(), "Initialize Qt Quick failed");
            QOpenGLFramebufferObject target(QSize(1280, 720), QOpenGLFramebufferObject::CombinedDepthStencil);
            window.setRenderTarget(QQuickRenderTarget::fromOpenGLTexture(target.texture(), target.size()));

            QQmlEngine qml;
            QQmlComponent component(&qml);
            component.loadFromModule(QStringLiteral("Qplay.Views"), QStringLiteral("PlayerScene"));
            if (component.isError())
                throw std::runtime_error(component.errorString().toStdString());
            std::unique_ptr<QQuickItem> root(qobject_cast<QQuickItem *>(component.createWithInitialProperties({
                {QStringLiteral("backend"), QVariant::fromValue(&backend)},
                {QStringLiteral("width"), 1280},
                {QStringLiteral("height"), 720}
            })));
            require(root != nullptr, "Create player scene failed");
            root->setParentItem(window.contentItem());

            const auto tick = [&]
            {
                app.processEvents();
                require(gl.makeCurrent(&surface), "Restore current OpenGL context failed");
                control.polishItems();
                control.beginFrame();
                control.sync();
                control.render();
                control.endFrame();
                QThread::msleep(16);
            };
            const auto wait = [&](const auto &condition, const char *message)
            {
                QElapsedTimer timer;
                timer.start();
                while (!condition() && timer.elapsed() < 15000)
                {
                    tick();
                    if (!backend.errorText().isEmpty())
                        throw std::runtime_error(backend.errorText().toStdString());
                }
                require(condition(), message);
            };

            {
                Qplay::PlayerController missing;
                missing.initialize(library + QStringLiteral(".missing"), QDir(profile).absoluteFilePath(QStringLiteral("missing")));
                wait([&] { return !missing.errorText().isEmpty(); }, "Missing engine fault did not arrive");
                require(!missing.engineReady(), "Missing engine became ready");
            }

            const QString incorrectLibrary = QDir(profile).absoluteFilePath(QStringLiteral("untrusted.dll"));
            QFile incorrectFile(incorrectLibrary);
            require(incorrectFile.open(QIODevice::WriteOnly), "Create engine fault fixture failed");
            incorrectFile.write("This is not the pinned engine");
            incorrectFile.close();
            {
                Qplay::PlayerController mismatch;
                mismatch.initialize(incorrectLibrary, QDir(profile).absoluteFilePath(QStringLiteral("mismatch")));
                wait([&] { return !mismatch.errorText().isEmpty(); }, "Engine hash fault did not arrive");
                require(!mismatch.engineReady(), "An untrusted engine became ready");
            }

            backend.initialize(library + QStringLiteral(".missing"), profile);
            backend.initialize(library, profile);
            wait([&] { return backend.engineReady(); }, "Initialize actual Rust/mpv session timed out");
            require(std::abs(backend.appliedAudioDelay() - 0.15) < 0.001,
                "Saved global audio delay was not applied to libmpv");
            backend.changeMute(true);
            wait([&] { return backend.muted(); }, "Muting before playback timed out");
            backend.openMedia(media);
            wait([&] { return backend.mediaLoaded() && backend.position() > 0.5; }, "Decode real media timed out");
            require(backend.duration() > 30, "Incorrect media duration");
            require(!backend.audioOutput().isEmpty(), "No audio output initialized");
            require(backend.audioTracks()->rowCount({}) > 0, "No typed audio track state");
            backend.changeShader(QStringLiteral("fsr"));
            wait([&] { return backend.effectiveShader() == QStringLiteral("fsr"); },
                "FSR shader did not reach libmpv");
            require(backend.gpuError().isEmpty(), "FSR shader returned an error");
            tick();
            require(target.toImage().save(evidencePrefix + QStringLiteral(".fsr.png")),
                "Save FSR playback composition failed");
            backend.changeShader(QStringLiteral("anime4k_a"));
            wait([&] { return backend.effectiveShader() == QStringLiteral("anime4k_a"); },
                "Anime4K shader did not reach libmpv");
            require(backend.gpuError().isEmpty(), "Anime4K shader returned an error");
            tick();
            require(target.toImage().save(evidencePrefix + QStringLiteral(".anime4k.png")),
                "Save Anime4K playback composition failed");
            {
                Qplay::PlayerController restored;
                restored.configure(preferences, QStringLiteral("en_US"));
                require(restored.shaderChoice() == QStringLiteral("anime4k_a")
                    && restored.framegenChoice() == QStringLiteral("off"),
                    "GPU choices did not persist");
            }

            backend.changeAudioDelay(0.2);
            wait([&] { return std::abs(backend.appliedAudioDelay() - 0.2) < 0.001; },
                "Live audio delay did not reach libmpv");
            backend.changeSpeed(1.25);
            wait([&] { return std::abs(backend.speed() - 1.25) < 0.001; },
                "Playback speed did not reach libmpv");

            const auto audioIndex = backend.audioTracks()->index(0, 0, {});
            const auto audioId = backend.audioTracks()->data(audioIndex, 256).toLongLong();
            backend.selectTrack(QStringLiteral("audio"), audioId);
            wait([&] { return backend.audioSelection() == QString::number(audioId); },
                "Exact audio selection did not reach libmpv");
            backend.selectAutomaticTrack(QStringLiteral("audio"));
            wait([&] { return backend.audioSelection() == QStringLiteral("auto"); },
                "Automatic audio selection did not reach libmpv");

            backend.openMedia(media);
            backend.stop();
            QElapsedTimer cancellation;
            cancellation.start();
            while (cancellation.elapsed() < 500)
                tick();
            require(!backend.mediaLoaded(), "A canceled media request became loaded");
            require(backend.errorText().isEmpty(), "A stale command affected the stopped player");
            backend.openMedia(media);
            wait([&] { return backend.mediaLoaded() && backend.position() > 0.5; }, "Reopening stopped media timed out");

            backend.changeVolume(37);
            wait([&] { return std::abs(backend.volume() - 37) < 0.001; }, "Volume command did not reach mpv");
            auto *pauseButton = root->findChild<QQuickItem *>(QStringLiteral("pauseButton"));
            require(pauseButton != nullptr, "Pause control unavailable");
            hover(window, *pauseButton);
            wait([&] { return pauseButton->isEnabled(); }, "Hover did not reveal transport controls");
            auto *seekSlider = root->findChild<QQuickItem *>(QStringLiteral("seekSlider"));
            auto *volumeSlider = root->findChild<QQuickItem *>(QStringLiteral("volumeSlider"));
            require(seekSlider != nullptr && seekSlider->height() >= 44
                && volumeSlider != nullptr && volumeSlider->height() >= 44,
                "Player sliders need 44-pixel input targets");
            click(window, *pauseButton);
            wait([&] { return backend.paused(); }, "QML pause input did not reach mpv");
            const auto pausedPosition = backend.position();
            QElapsedTimer pauseTimer;
            pauseTimer.start();
            while (pauseTimer.elapsed() < 500)
                tick();
            require(std::abs(backend.position() - pausedPosition) < 0.05, "Playback clock advanced while paused");

            backend.seek(20);
            wait([&] { return std::abs(backend.position() - 20) < 0.05; }, "Exact seek timed out");
            require(backend.paused(), "Seeking lost paused state");

            const QString subtitles = QDir(profile).absoluteFilePath(QStringLiteral("English.srt"));
            QFile subtitleFile(subtitles);
            require(subtitleFile.open(QIODevice::WriteOnly), "Create legal subtitle fixture failed");
            subtitleFile.write("1\n00:00:00,000 --> 00:00:45,000\nNative Rust player subtitle\n");
            subtitleFile.close();
            backend.addSubtitle(subtitles);
            wait([&] { return selectedSubtitle(backend.subtitleTracks()) >= 0; }, "External subtitle track did not arrive");
            const auto subtitleId = selectedSubtitle(backend.subtitleTracks());
            backend.disableSubtitles();
            wait([&] { return selectedSubtitle(backend.subtitleTracks()) == -1
                && backend.subtitleSelection() == QStringLiteral("no"); }, "Subtitle disable did not reach mpv");
            backend.selectAutomaticTrack(QStringLiteral("sub"));
            wait([&] { return backend.subtitleSelection() == QStringLiteral("auto"); },
                "Automatic subtitles did not reach libmpv");
            backend.selectTrack(QStringLiteral("sub"), subtitleId);
            wait([&] { return selectedSubtitle(backend.subtitleTracks()) == subtitleId; }, "Exact subtitle selection did not reach mpv");

            click(window, *pauseButton);
            wait([&] { return !backend.paused() && backend.position() > 21; }, "QML resume did not advance playback");
            click(window, *pauseButton);
            wait([&] { return backend.paused(); }, "QML pause before visual inspection failed");
            backend.seek(20);
            wait([&] { return std::abs(backend.position() - 20) < 0.05; },
                "Seek to the visual inspection frame failed");
            backend.changeShader(QStringLiteral("off"));
            wait([&] { return backend.effectiveShader() == QStringLiteral("off"); },
                "Disabling the shader did not reach libmpv");
            for (int frame = 0; frame < 4; ++frame)
                tick();
            const auto plainFrame = target.toImage().copy(QRect(100, 120, 800, 380));
            backend.changeShader(QStringLiteral("anime4k_a"));
            wait([&] { return backend.effectiveShader() == QStringLiteral("anime4k_a"); },
                "Restoring Anime4K did not reach libmpv");
            for (int frame = 0; frame < 4; ++frame)
                tick();
            const auto processedFrame = target.toImage().copy(QRect(100, 120, 800, 380));
            require(plainFrame != processedFrame, "Anime4K did not change the rendered video frame");
            hover(window, *pauseButton);
            tick();
            require(target.toImage().save(evidencePrefix + QStringLiteral(".png")), "Save actual Qt/mpv composition failed");

            auto *settingsButton = root->findChild<QQuickItem *>(QStringLiteral("settingsButton"));
            auto *playbackMenu = root->findChild<QObject *>(QStringLiteral("playbackMenu"));
            require(settingsButton != nullptr && playbackMenu != nullptr, "Playback settings controls unavailable");
            hover(window, *settingsButton);
            click(window, *settingsButton);
            wait([&] { return playbackMenu->property("opened").toBool(); },
                "The playback menu did not open from the centered transport");
            const bool playbackMenuOpened = playbackMenu->property("opened").toBool();
            hover(window, *root);
            QElapsedTimer menuAnimation;
            menuAnimation.start();
            while (menuAnimation.elapsed() < 250)
                tick();
            require(target.toImage().save(evidencePrefix + QStringLiteral(".menu.png")),
                "Save actual playback menu composition failed");
            auto *popupContent = qvariant_cast<QQuickItem *>(playbackMenu->property("contentItem"));
            require(popupContent != nullptr, "Playback menu content unavailable");
            auto *speedRow = findVisualItem(popupContent, QStringLiteral("speedMenuRow"));
            require(speedRow != nullptr, "Speed menu row unavailable");
            click(window, *speedRow);
            wait([&] { return playbackMenu->property("page").toInt() == 3; },
                "The speed row did not open its own view");
            const bool speedSubmenuOpened = playbackMenu->property("page").toInt() == 3;
            tick();
            require(target.toImage().save(evidencePrefix + QStringLiteral(".speed.png")),
                "Save actual speed controls failed");
            auto *speedSlider = findVisualItem(popupContent, QStringLiteral("speedSlider"));
            auto *speedPreset = findVisualItem(popupContent, QStringLiteral("speedPreset2"));
            require(speedSlider != nullptr && speedPreset != nullptr, "Speed slider or presets unavailable");
            require(std::abs(speedSlider->property("from").toDouble() - 0.25) < 0.001
                && std::abs(speedSlider->property("to").toDouble() - 4) < 0.001
                && std::abs(speedSlider->property("stepSize").toDouble() - 0.05) < 0.001,
                "Speed slider has the wrong range or precision");
            click(window, *speedPreset);
            wait([&] { return std::abs(backend.speed() - 2) < 0.001; },
                "QML speed preset did not reach libmpv");

            playbackMenu->setProperty("page", 0);
            tick();
            auto *fxRow = findVisualItem(popupContent, QStringLiteral("fxMenuRow"));
            require(fxRow != nullptr, "FX menu row unavailable");
            click(window, *fxRow);
            wait([&] { return playbackMenu->property("page").toInt() == 4; },
                "The FX row did not open its own view");
            tick();
            require(target.toImage().save(evidencePrefix + QStringLiteral(".fx.png")),
                "Save actual FX controls failed");

            require(QMetaObject::invokeMethod(playbackMenu, "close"), "Close playback menu failed");
            wait([&] { return !playbackMenu->property("opened").toBool(); },
                "Playback menu did not close");
            QQmlComponent settingsComponent(&qml);
            settingsComponent.loadFromModule(QStringLiteral("Qplay.Views"), QStringLiteral("SettingsPanel"));
            if (settingsComponent.isError())
                throw std::runtime_error(settingsComponent.errorString().toStdString());
            std::unique_ptr<QObject> settingsPanel(settingsComponent.createWithInitialProperties({
                {QStringLiteral("backend"), QVariant::fromValue(&backend)},
                {QStringLiteral("host"), QVariant::fromValue(root.get())},
                {QStringLiteral("russian"), false},
                {QStringLiteral("dark"), false}
            }));
            require(settingsPanel != nullptr, "Create advanced settings panel failed");
            backend.changeAppearance(QStringLiteral("light"));
            require(QMetaObject::invokeMethod(settingsPanel.get(), "open"), "Open advanced settings failed");
            wait([&] { return settingsPanel->property("opened").toBool(); },
                "Advanced settings did not open");
            wait([&] { return !backend.subtitleModels().isEmpty()
                || backend.subtitleStatus() == QStringLiteral("unavailable"); },
                "Subtitle model catalog did not finish");
            auto *settingsContent = qvariant_cast<QQuickItem *>(settingsPanel->property("contentItem"));
            require(settingsContent != nullptr, "Advanced settings content unavailable");
            auto *translationSwitch = findVisualItem(settingsContent, QStringLiteral("subtitleTranslationSwitch"));
            auto *targetEnglish = findVisualItem(settingsContent, QStringLiteral("subtitleTargeten"));
            auto *targetRussian = findVisualItem(settingsContent, QStringLiteral("subtitleTargetru"));
            require(translationSwitch != nullptr && targetEnglish != nullptr && targetRussian != nullptr,
                "Subtitle translation controls unavailable");
            click(window, *targetEnglish);
            wait([&] { return backend.subtitleTarget() == QStringLiteral("en"); },
                "English subtitle target did not reach the controller");
            click(window, *targetRussian);
            wait([&] { return backend.subtitleTarget() == QStringLiteral("ru"); },
                "Russian subtitle target did not reach the controller");
            tick();
            require(target.toImage().save(evidencePrefix + QStringLiteral(".settings-light.png")),
                "Save light advanced settings failed");
            auto *settingsScroll = findVisualItem(settingsContent, QStringLiteral("settingsScroll"));
            require(settingsScroll != nullptr, "Scrollable subtitle settings unavailable");
            auto *settingsFlickable = qvariant_cast<QQuickItem *>(settingsScroll->property("contentItem"));
            require(settingsFlickable != nullptr && settingsFlickable->property("contentY").isValid(),
                "Settings content is not scrollable");
            const auto scrollLimit = settingsFlickable->property("contentHeight").toDouble()
                - settingsFlickable->height();
            require(scrollLimit > 0 && settingsFlickable->setProperty("contentY", scrollLimit),
                "Settings did not expose its lower categories");
            tick();
            require(target.toImage().save(evidencePrefix + QStringLiteral(".settings-720-bottom.png")),
                "Save scrolled subtitle settings failed");
            auto *modelChoice = findVisualItem(settingsContent, QStringLiteral("subtitleModel"));
            auto *effortChoice = findVisualItem(settingsContent, QStringLiteral("subtitleEffort"));
            require(modelChoice != nullptr && effortChoice != nullptr
                && modelChoice->isEnabled() && !effortChoice->isEnabled(),
                "Unavailable model did not leave available models selectable");
            auto *modelText = qvariant_cast<QQuickItem *>(modelChoice->property("contentItem"));
            auto *effortText = qvariant_cast<QQuickItem *>(effortChoice->property("contentItem"));
            require(modelChoice->property("currentIndex").toInt() == 0
                && effortChoice->property("currentIndex").toInt() == 0
                && modelChoice->property("savedChoiceUnavailable").toBool()
                && effortChoice->property("savedChoiceUnavailable").toBool()
                && modelText != nullptr && effortText != nullptr
                && modelText->property("text").toString().contains(QStringLiteral("retired-model"))
                && effortText->property("text").toString().contains(QStringLiteral("retired-effort")),
                "Unavailable saved choices were displayed as Auto");
            click(window, *modelChoice);
            auto *modelPopup = qvariant_cast<QObject *>(modelChoice->property("popup"));
            wait([&] { return modelPopup != nullptr && modelPopup->property("opened").toBool(); },
                "Model selector did not open from pointer input");
            tick();
            require(target.toImage().save(evidencePrefix + QStringLiteral(".settings-models.png")),
                "Save model choices failed");
            auto *modelOptions = qvariant_cast<QQuickItem *>(modelPopup->property("contentItem"));
            require(modelOptions != nullptr
                && modelOptions->mapToScene(QPointF(0, 0)).y() >= 0
                && modelOptions->mapToScene(QPointF(0, modelOptions->height())).y() <= root->height(),
                "Model selector extends outside the player");
            auto *firstModel = findVisualItem(modelOptions, QStringLiteral("subtitleModelOption2"));
            require(firstModel != nullptr, "First model choice unavailable");
            click(window, *firstModel);
            wait([&] { return backend.subtitleModel() == backend.subtitleModels().first(); },
                "Model choice did not reach the controller");
            require(!modelPopup->property("opened").toBool(), "Model selector did not close after selection");
            require(effortChoice->isEnabled(), "Available model did not enable effort selection");

            require(savedPreferences.open(QIODevice::ReadOnly), "Read selected subtitle model failed");
            settings = QJsonDocument::fromJson(savedPreferences.readAll()).object();
            savedPreferences.close();
            translation = settings.value(QStringLiteral("subtitle_translation")).toObject();
            translation.insert(QStringLiteral("effort"), QStringLiteral("retired-effort"));
            settings.insert(QStringLiteral("subtitle_translation"), translation);
            require(savedPreferences.open(QIODevice::WriteOnly | QIODevice::Truncate)
                    && savedPreferences.write(QJsonDocument(settings).toJson()) > 0,
                "Save unavailable effort with an available model failed");
            savedPreferences.close();
            backend.configure(preferences, QStringLiteral("en_US"));
            tick();
            require(effortChoice->isEnabled()
                && effortChoice->property("savedChoiceUnavailable").toBool()
                && effortText->property("text").toString().contains(QStringLiteral("retired-effort")),
                "Unavailable saved effort was displayed as Auto with an available model");
            click(window, *effortChoice);
            auto *effortPopup = qvariant_cast<QObject *>(effortChoice->property("popup"));
            wait([&] { return effortPopup != nullptr && effortPopup->property("opened").toBool(); },
                "Effort selector did not open from pointer input");
            auto *effortOptions = qvariant_cast<QQuickItem *>(effortPopup->property("contentItem"));
            require(effortOptions != nullptr
                && effortOptions->mapToScene(QPointF(0, 0)).y() >= 0
                && effortOptions->mapToScene(QPointF(0, effortOptions->height())).y() <= root->height(),
                "Effort selector extends outside the player");
            auto *firstEffort = findVisualItem(effortOptions, QStringLiteral("subtitleEffortOption2"));
            require(firstEffort != nullptr, "First effort choice unavailable");
            click(window, *firstEffort);
            wait([&] { return backend.subtitleEffort() == backend.subtitleEfforts().first(); },
                "Effort choice did not reach the controller");
            require(!effortPopup->property("opened").toBool(), "Effort selector did not close after selection");
            require(settingsFlickable->setProperty("contentY", 0.0), "Reset settings scroll failed");
            QOpenGLFramebufferObject largeTarget(QSize(1440, 900), QOpenGLFramebufferObject::CombinedDepthStencil);
            window.setGeometry(0, 0, 1440, 900);
            root->setSize(QSizeF(1440, 900));
            window.setRenderTarget(QQuickRenderTarget::fromOpenGLTexture(largeTarget.texture(), largeTarget.size()));
            tick();
            require(largeTarget.toImage().save(evidencePrefix + QStringLiteral(".settings-1440-light.png")),
                "Save 1440x900 subtitle settings failed");
            window.setGeometry(0, 0, 1280, 720);
            root->setSize(QSizeF(1280, 720));
            window.setRenderTarget(QQuickRenderTarget::fromOpenGLTexture(target.texture(), target.size()));
            backend.changeAppearance(QStringLiteral("dark"));
            settingsPanel->setProperty("dark", true);
            tick();
            require(target.toImage().save(evidencePrefix + QStringLiteral(".settings-dark.png")),
                "Save dark advanced settings failed");
            click(window, *translationSwitch);
            wait([&] { return backend.subtitleTranslationEnabled(); },
                "Global subtitle translation switch did not enable the service");
            tick();
            click(window, *translationSwitch);
            wait([&] { return !backend.subtitleTranslationEnabled()
                && selectedSubtitle(backend.subtitleTracks()) == subtitleId; },
                "Disabling translation did not restore the original subtitle track");
            require(QThreadPool::globalInstance()->waitForDone(15000),
                "Canceled subtitle worker did not stop");
            require(QMetaObject::invokeMethod(settingsPanel.get(), "close"), "Close advanced settings failed");
            backend.togglePause();
            wait([&] { return !backend.paused(); }, "Resume after settings inspection failed");

            const auto children = childProcessCount();
            require(children == 0, "Playback spawned a child runtime");
            evidence = {
                {QStringLiteral("fileLoaded"), backend.mediaLoaded()},
                {QStringLiteral("position"), backend.position()},
                {QStringLiteral("duration"), backend.duration()},
                {QStringLiteral("volume"), backend.volume()},
                {QStringLiteral("audioOutput"), backend.audioOutput()},
                {QStringLiteral("hardwareDecoder"), backend.hardwareDecoder()},
                {QStringLiteral("audioTrackCount"), backend.audioTracks()->rowCount({})},
                {QStringLiteral("subtitleTrackCount"), backend.subtitleTracks()->rowCount({})},
                {QStringLiteral("selectedSubtitle"), subtitleId},
                {QStringLiteral("speed"), backend.speed()},
                {QStringLiteral("globalAudioDelay"), backend.audioDelay()},
                {QStringLiteral("libmpvAudioDelay"), backend.appliedAudioDelay()},
                {QStringLiteral("ownedChildren"), children},
                {QStringLiteral("effectiveShader"), backend.effectiveShader()},
                {QStringLiteral("anime4kChangedRenderedFrame"), true},
                {QStringLiteral("framegenChoice"), backend.framegenChoice()},
                {QStringLiteral("pauseSeekResume"), true},
                {QStringLiteral("playbackMenuOpened"), playbackMenuOpened},
                {QStringLiteral("speedSubmenuOpened"), speedSubmenuOpened},
                {QStringLiteral("fxSubmenuOpened"), true},
                {QStringLiteral("speedPresetApplied"), std::abs(backend.speed() - 2) < 0.001},
                {QStringLiteral("settingsLightRendered"), true},
                {QStringLiteral("settingsDarkRendered"), true},
                {QStringLiteral("subtitleTargetSelection"), true},
                {QStringLiteral("subtitleToggleCancellation"), true},
                {QStringLiteral("unavailableSubtitleModel"), true},
                {QStringLiteral("unavailableSubtitleEffort"), true},
                {QStringLiteral("subtitleModelCount"), backend.subtitleModels().size()},
                {QStringLiteral("pendingCatalogCancellation"), pendingCatalogCancellation},
                {QStringLiteral("stopReopen"), true},
                {QStringLiteral("preferencesPersisted"), true},
                {QStringLiteral("engineFaults"), true},
                {QStringLiteral("startupReplacement"), true},
                {QStringLiteral("input"), QStringLiteral("Generated Qt mouse events on the offscreen scene")},
                {QStringLiteral("audio"), QStringLiteral("Actual audio output initialized, muted for host workflow")}
            };

            backend.openMedia(secondMedia);
            wait([&] { return backend.mediaLoaded() && backend.position() > 0.5; },
                "Second legal video path did not load");
            require(std::abs(backend.appliedAudioDelay() - 0.2) < 0.001,
                "Global audio delay changed on the second file");
            evidence[QStringLiteral("secondFileAudioDelay")] = backend.appliedAudioDelay();

            backend.initialize(library, profile);
            tick();
            wait([&] { return backend.engineReady(); }, "Restarting the mpv session timed out");
            require(std::abs(backend.appliedAudioDelay() - 0.2) < 0.001,
                "Global audio delay was not restored into a new libmpv session");
            wait([&] { return backend.mediaLoaded() && backend.position() > 0.5; },
                "The second file did not reopen after a native session restart");
            evidence[QStringLiteral("reopenedPosition")] = backend.position();
            evidence[QStringLiteral("restartAudioDelay")] = backend.appliedAudioDelay();

            backend.stop();
            wait([&] { return !backend.mediaLoaded() && backend.position() == 0 && backend.duration() == 0; }, "Stopping actual mpv playback failed");
            if (qEnvironmentVariableIsSet("QPLAY_NATIVE_FRAMEGEN_ACCEPTANCE"))
            {
                const auto waitGpu = [&](const auto &condition, qint64 timeout, const char *message)
                {
                    QElapsedTimer timer;
                    timer.start();
                    QString previousStatus;
                    while (!condition() && timer.elapsed() < timeout)
                    {
                        tick();
                        const auto status = backend.framegenStatus();
                        if (status != previousStatus)
                        {
                            std::fprintf(stderr, "Frame generation: %s\n", status.toUtf8().constData());
                            std::fflush(stderr);
                            previousStatus = status;
                        }
                        if (!backend.gpuError().isEmpty())
                            throw std::runtime_error(backend.gpuError().toStdString());
                    }
                    require(condition(), message);
                };

                backend.changeSpeed(1);
                wait([&] { return std::abs(backend.speed() - 1) < 0.001; },
                    "Normal playback speed did not return");
                backend.changeShader(QStringLiteral("auto"));
                backend.changeContent(QStringLiteral("movie"));
                backend.changeFramegen(QStringLiteral("on"));
                backend.openMedia(secondMedia);
                wait([&] { return backend.mediaLoaded(); }, "Frame generation video did not load");
                if (!backend.paused())
                    backend.togglePause();
                wait([&] { return backend.paused(); }, "Pause before graph preparation failed");
                waitGpu([&] { return backend.framegenStatus() == QStringLiteral("prepared")
                    || backend.framegenStatus() == QStringLiteral("starting")
                    || backend.framegenStatus() == QStringLiteral("active"); }, 30 * 60 * 1000,
                    "Frame generation graph preparation timed out");
                if (backend.framegenStatus() == QStringLiteral("prepared"))
                {
                    backend.openMedia(secondMedia);
                    wait([&] { return backend.mediaLoaded(); }, "Prepared video did not reopen");
                }
                if (backend.paused())
                    backend.togglePause();
                waitGpu([&] { return backend.framegenStatus() == QStringLiteral("active"); },
                    2 * 60 * 1000, "Frame generation output was not verified");
                require(backend.gpuError().isEmpty(), "Frame generation returned an error");
                tick();
                const QImage firstVideo = target.toImage().copy(0, 88, 1280, 500);
                const double firstPosition = backend.position();
                QElapsedTimer playback;
                playback.start();
                while (playback.elapsed() < 5000)
                    tick();
                const double playbackAdvance = backend.position() - firstPosition;
                require(playbackAdvance >= 4.0 && backend.framegenStatus() == QStringLiteral("active"),
                    "Frame generation did not sustain real-time playback");
                require(target.toImage().copy(0, 88, 1280, 500) != firstVideo,
                    "Frame generation did not render changing video");
                const auto observation = backend.framegenObservation();
                require(observation.source_fps > 0 && observation.target_fps > observation.source_fps,
                    "Frame generation did not increase the source frame rate");
                require(std::abs(observation.measured_fps - observation.target_fps) <= observation.target_fps * 0.03,
                    "The rendered filter did not produce the target frame rate");
                require(observation.output_width > 0 && observation.output_height > 0
                    && observation.filter_name == "@mpv-framegen",
                    "The rendered frame generation output was not present");
                require(target.toImage().save(evidencePrefix + QStringLiteral(".framegen.png")),
                    "Save generated-frame playback composition failed");
                evidence[QStringLiteral("framegenActive")] = true;
                evidence[QStringLiteral("framegenPlaybackAdvanceSeconds")] = playbackAdvance;
                evidence[QStringLiteral("framegenWallSeconds")] = playback.elapsed() / 1000.0;
                evidence[QStringLiteral("framegenRenderedVideoChanged")] = true;
                evidence[QStringLiteral("framegenChoice")] = backend.framegenChoice();
                evidence[QStringLiteral("framegenStatus")] = backend.framegenStatus();
                evidence[QStringLiteral("framegenSourceFps")] = observation.source_fps;
                evidence[QStringLiteral("framegenTargetFps")] = observation.target_fps;
                evidence[QStringLiteral("framegenMeasuredFps")] = observation.measured_fps;
                evidence[QStringLiteral("framegenOutputWidth")] = static_cast<int>(observation.output_width);
                evidence[QStringLiteral("framegenOutputHeight")] = static_cast<int>(observation.output_height);
                evidence[QStringLiteral("framegenFilter")] = QString::fromUtf8(
                    observation.filter_name.data(), observation.filter_name.size());
                backend.changeShader(QStringLiteral("anime4k_a"));
                waitGpu([&] { return backend.framegenStatus() == QStringLiteral("off")
                    && backend.framegenChoice() == QStringLiteral("off")
                    && backend.effectiveShader() == QStringLiteral("anime4k_a"); }, 15 * 1000,
                    "Manual Anime4K did not replace active frame generation");
                require(backend.framegenObservation().filter_name.empty(),
                    "Manual Anime4K left the frame generation filter active");
                evidence[QStringLiteral("manualShaderStoppedFramegen")] = true;
                backend.stop();
                wait([&] { return !backend.mediaLoaded(); }, "Stopping generated-frame playback failed");
                Qplay::PlayerController restored;
                restored.configure(preferences, QStringLiteral("en_US"));
                require(restored.shaderChoice() == QStringLiteral("anime4k_a")
                    && restored.framegenChoice() == QStringLiteral("off"),
                    "Manual GPU choices were not saved together");
                evidence[QStringLiteral("manualGpuChoicesPersisted")] = true;
                backend.changeFramegen(QStringLiteral("auto"));
                require(backend.shaderChoice() == QStringLiteral("auto")
                    && backend.framegenChoice() == QStringLiteral("auto"),
                    "Automatic frame generation did not restore automatic shader choice");
                backend.changeShader(QStringLiteral("anime4k_a"));
                require(backend.framegenChoice() == QStringLiteral("off"),
                    "Manual Anime4K did not disable automatic frame generation");
                evidence[QStringLiteral("autoFramegenResolvedManualShader")] = true;
                backend.changeFramegen(QStringLiteral("on"));
                require(backend.shaderChoice() == QStringLiteral("fsr")
                    && backend.framegenChoice() == QStringLiteral("on"),
                    "Manual frame generation did not select compatible FSR");
                backend.openMedia(secondMedia);
                wait([&] { return backend.mediaLoaded(); }, "Frame generation did not reopen after manual shader");
                waitGpu([&] { return backend.framegenStatus() == QStringLiteral("active"); },
                    2 * 60 * 1000, "Frame generation did not resume after manual Anime4K");
                evidence[QStringLiteral("framegenRestoredAfterManualShader")] = true;
                backend.stop();
                wait([&] { return !backend.mediaLoaded(); }, "Stopping restored frame generation failed");
            }
            root.reset();
            gl.makeCurrent(&surface);
            control.invalidate();
        }

        // Closing a QObject during accepted startup must leave no live callback owner.
        for (int attempt = 0; attempt < 4; ++attempt)
        {
            auto *closing = new Qplay::PlayerController;
            closing->initialize(library, QDir(profile).absoluteFilePath(QStringLiteral("cancel-%1").arg(attempt)));
            delete closing;
        }
        QElapsedTimer shutdown;
        shutdown.start();
        QThreadPool::globalInstance()->waitForDone();
        evidence[QStringLiteral("shutdownMs")] = shutdown.elapsed();
        evidence[QStringLiteral("closeDuringStartup")] = true;
        require(childProcessCount() == 0, "Owned child survived shutdown");
    }
    catch (const std::exception &error)
    {
        evidence[QStringLiteral("error")] = QString::fromUtf8(error.what());
        std::fprintf(stderr, "%s\n", error.what());
        exitCode = 1;
    }
    QThreadPool::globalInstance()->waitForDone();
    evidence[QStringLiteral("exitCode")] = exitCode;
    QFile output(evidencePrefix + QStringLiteral(".json"));
    require(output.open(QIODevice::WriteOnly), "Write evidence failed");
    output.write(QJsonDocument(evidence).toJson());
    return exitCode;
}
