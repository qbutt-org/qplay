#include <cstdio>
#include <cstring>
#include <exception>
#include <memory>

#include <QByteArray>
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QIcon>
#include <QLocale>
#include <QObject>
#include <QQmlApplicationEngine>
#include <QQmlError>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QScreen>
#include <QStandardPaths>
#include <QStringList>
#include <QTextStream>
#include <QThreadPool>

#include "qplay_native/src/controller.cxxqt.h"

int main(int argc, char *argv[])
{
    bool metadataQuery = false;
    for (int i = 1; i < argc; ++i)
    {
        if (std::strcmp(argv[i], "--") == 0)
            break;
        if ((std::strcmp(argv[i], "--version-json") == 0)
            || (std::strcmp(argv[i], "--runtime-info-json") == 0))
        {
            metadataQuery = true;
            break;
        }
    }

    std::unique_ptr<QCoreApplication> app;
    if (metadataQuery)
        app = std::make_unique<QCoreApplication>(argc, argv);
    else
    {
        QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
        QQuickStyle::setStyle(QStringLiteral("Basic"));
        app = std::make_unique<QGuiApplication>(argc, argv);
        QGuiApplication::setWindowIcon(QIcon(QStringLiteral(":/assets/qplay.ico")));
    }

    QString media;
    QString profile;
    QStringList playbackArguments;
    bool versionJson = false;
    bool runtimeInfoJson = false;
    bool positional = false;
    const QStringList arguments = app->arguments();
    for (int i = 1; i < arguments.size(); ++i)
    {
        const QString &argument = arguments.at(i);
        if (!positional && argument == QStringLiteral("--"))
            positional = true;
        else if (!positional && argument.startsWith(QStringLiteral("--profile-dir=")))
            profile = argument.sliced(14);
        else if (!positional && argument == QStringLiteral("--version-json"))
            versionJson = true;
        else if (!positional && argument == QStringLiteral("--runtime-info-json"))
            runtimeInfoJson = true;
        else if (!positional && argument.startsWith(u'-'))
            playbackArguments.append(argument);
        else if (!media.isEmpty())
        {
            std::fprintf(stderr, "Expected at most one media source.\n");
            return 2;
        }
        else
            media = argument;
    }
    if ((versionJson && runtimeInfoJson) || ((versionJson || runtimeInfoJson)
        && (!media.isEmpty() || !playbackArguments.isEmpty())))
    {
        std::fprintf(stderr, "Metadata query cannot include media or another query.\n");
        return 2;
    }
    app->setApplicationName(QStringLiteral("qplay"));
    const QString installDirectory = QCoreApplication::applicationDirPath();
    const QString library = installDirectory + QStringLiteral("/engine/libmpv-2.dll");
    const bool portable = QFileInfo::exists(installDirectory + QStringLiteral("/qplay.portable"));
    QString configuration;
    if (profile.isEmpty())
    {
        configuration = portable
            ? installDirectory + QStringLiteral("/userdata/config")
            : QStandardPaths::writableLocation(QStandardPaths::AppConfigLocation);
        profile = portable
            ? installDirectory + QStringLiteral("/userdata/data")
            : QStandardPaths::writableLocation(QStandardPaths::AppLocalDataLocation);
    }
    else
    {
        configuration = QDir(profile).absoluteFilePath(QStringLiteral("config"));
        profile = QDir(profile).absoluteFilePath(QStringLiteral("data"));
    }

    if (versionJson || runtimeInfoJson)
    {
        try
        {
            rust::String output;
            if (runtimeInfoJson)
            {
                const QByteArray libraryUtf8 = library.toUtf8();
                const QByteArray profileUtf8 = profile.toUtf8();
                const QByteArray configurationUtf8 = configuration.toUtf8();
                output = Qplay::runtime_info_json(
                    rust::Str(libraryUtf8.constData(), libraryUtf8.size()),
                    rust::Str(profileUtf8.constData(), profileUtf8.size()),
                    rust::Str(configurationUtf8.constData(), configurationUtf8.size()));
            }
            else
                output = Qplay::version_json();
            std::fwrite(output.data(), 1, output.size(), stdout);
            std::fputc('\n', stdout);
            return 0;
        }
        catch (const std::exception &error)
        {
            std::fprintf(stderr, "%s\n", error.what());
            return 1;
        }
    }

    qputenv("QPLAY_MEDIA_RUNTIME", QDir(profile).absoluteFilePath(QStringLiteral("framegen")).toUtf8());

    int result;
    {
        Qplay::PlayerController backend;
        const QString launchError = backend.configureLaunch(playbackArguments);
        if (!launchError.isEmpty())
        {
            std::fprintf(stderr, "%s\n", launchError.toUtf8().constData());
            return 2;
        }
        backend.configure(configuration + QStringLiteral("/preferences.json"), QLocale::system().name());
        backend.configureInstallation(profile, installDirectory, portable);
        QObject::connect(&backend, &Qplay::PlayerController::shutdownRequested,
            app.get(), [&app] { app->quit(); });
        QQmlApplicationEngine engine;
        QStringList startupWarnings;
        QObject::connect(&engine, &QQmlEngine::warnings, &engine,
            [&](const QList<QQmlError> &warnings) {
                for (const QQmlError &warning : warnings)
                    startupWarnings.append(warning.toString());
            });
        engine.setInitialProperties({{QStringLiteral("backend"), QVariant::fromValue(&backend)}});
        engine.loadFromModule(QStringLiteral("Qplay.Views"), QStringLiteral("Main"));
        if (engine.rootObjects().isEmpty())
        {
            QDir().mkpath(profile);
            QFile diagnostics(QDir(profile).filePath(QStringLiteral("startup-error.log")));
            if (diagnostics.open(QIODevice::WriteOnly | QIODevice::Text))
            {
                QTextStream output(&diagnostics);
                for (const QString &warning : startupWarnings)
                    output << warning << '\n';
            }
            return 1;
        }
        QMetaObject::Connection refreshConnection;
        auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());
        if (window)
        {
            const auto syncScreen = [&backend, &refreshConnection](QScreen *screen)
            {
                QObject::disconnect(refreshConnection);
                backend.setDisplayRate(screen ? screen->refreshRate() : 0);
                if (screen)
                    refreshConnection = QObject::connect(screen, &QScreen::refreshRateChanged,
                        &backend, [&backend](qreal rate) { backend.setDisplayRate(rate); });
            };
            QObject::connect(window, &QQuickWindow::screenChanged, &backend, syncScreen);
            syncScreen(window->screen());
        }
        if (!media.isEmpty())
            backend.openMedia(media);
        backend.initialize(library, profile);
        result = app->exec();
    }
    QThreadPool::globalInstance()->waitForDone();
    return result;
}
