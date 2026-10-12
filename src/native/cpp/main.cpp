#include <cstdio>

#include <QDir>
#include <QFileInfo>
#include <QGuiApplication>
#include <QIcon>
#include <QLocale>
#include <QQmlApplicationEngine>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QStandardPaths>
#include <QThreadPool>

#include "qplay_native/src/controller.cxxqt.h"

int main(int argc, char *argv[])
{
    QString media;
    QString profile;
    for (int i = 1; i < argc; ++i)
    {
        const QString argument = QString::fromUtf8(argv[i]);
        if (argument.startsWith(QStringLiteral("--profile-dir=")))
            profile = argument.sliced(14);
        else if (argument.startsWith(u'-') || !media.isEmpty())
        {
            std::fprintf(stderr, "Expected at most one media file and optional --profile-dir.\n");
            return 2;
        }
        else
            media = argument;
    }
    QQuickWindow::setGraphicsApi(QSGRendererInterface::OpenGL);
    QQuickStyle::setStyle(QStringLiteral("Basic"));
    QGuiApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("qplay"));
    app.setWindowIcon(QIcon(QStringLiteral(":/assets/qplay.ico")));
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

    int result;
    {
        Qplay::PlayerController backend;
        backend.configure(configuration + QStringLiteral("/preferences.json"), QLocale::system().name());
        QQmlApplicationEngine engine;
        engine.setInitialProperties({{QStringLiteral("backend"), QVariant::fromValue(&backend)}});
        engine.loadFromModule(QStringLiteral("Qplay.Views"), QStringLiteral("Main"));
        if (engine.rootObjects().isEmpty())
            return 1;
        if (!media.isEmpty())
            backend.openMedia(media);
        backend.initialize(library, profile);
        result = app.exec();
    }
    QThreadPool::globalInstance()->waitForDone();
    return result;
}
