#pragma once

#include <stdexcept>

#include <QDir>
#include <QString>
#include <QTemporaryDir>

inline QString testIpcName(const QString &name)
{
#ifdef Q_OS_WIN
    return name;
#else
    static const QTemporaryDir directory(QDir::tempPath() + QStringLiteral("/qplay-ipc-XXXXXX"));
    if (!directory.isValid())
        throw std::runtime_error("Create isolated IPC directory failed");
    return directory.filePath(name);
#endif
}

inline QString testIpcOption(const QString &name)
{
#ifdef Q_OS_WIN
    return QStringLiteral("--input-ipc-server=\\\\.\\pipe\\") + name;
#else
    return QStringLiteral("--input-ipc-server=") + name;
#endif
}
