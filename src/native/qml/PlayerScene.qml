pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Qplay.Player

Item {
    id: root
    required property PlayerController backend
    property bool russian: false
    property bool settingsOpen: false
    property bool transportVisible: true
    readonly property string iconRoot: "qrc:/qt/qml/Qplay/Views/assets/icons/"
    readonly property bool controlsShown: backend.state !== Playback.Playing || transportVisible || settingsOpen || openMenu.opened || playbackMenu.opened || topHover.hovered || bottomHover.hovered
    readonly property int positionSeconds: Math.floor(backend.position)
    readonly property int durationSeconds: Math.floor(backend.duration)

    signal openFileRequested
    signal openLinkRequested
    signal settingsRequested
    signal fullscreenRequested

    function clock(seconds) {
        const hours = Math.floor(seconds / 3600);
        const minutes = Math.floor((seconds % 3600) / 60);
        const remainder = (seconds % 60).toString().padStart(2, "0");
        return hours > 0 ? hours + ":" + minutes.toString().padStart(2, "0") + ":" + remainder : minutes + ":" + remainder;
    }

    function revealControls() {
        transportVisible = true;
        if (hideTimer.running)
            hideTimer.restart();
    }

    Rectangle {
        anchors.fill: parent
        color: "#202123"
    }

    Loader {
        anchors.fill: parent
        active: root.backend.engineReady
        sourceComponent: VideoSurface {
            backend: root.backend
        }
    }

    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: root.backend.mediaLoaded ? Qt.LeftButton : Qt.NoButton
        cursorShape: root.controlsShown ? Qt.ArrowCursor : Qt.BlankCursor
        onEntered: root.revealControls()
        onPositionChanged: root.revealControls()
        onClicked: root.backend.togglePause()
    }

    TextMetrics {
        id: subtitleMeasure
        font.family: "Arial"
        font.pixelSize: Math.round(root.height * 0.045)
        text: root.backend.subtitleText.split("\n").reduce((longest, line) => line.length > longest.length ? line : longest, "")
    }

    Item {
        id: subtitleDrag
        visible: root.backend.subtitleText.length > 0 && !root.settingsOpen && !openMenu.opened && !playbackMenu.opened
        width: Math.min(root.width - 48, Math.max(80, subtitleMeasure.advanceWidth + 36))
        height: subtitleMeasure.height * Math.min(4, root.backend.subtitleText.split("\n").length) + 16
        x: (root.width - width) / 2
        y: Math.max(0, Math.min(root.height - height, root.height * root.backend.subtitlePosition / 100 - height * 1.35))

        MouseArea {
            id: subtitleMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: pressed && dragged ? Qt.ClosedHandCursor : Qt.OpenHandCursor
            property real pressY: 0
            property real startPosition: 0
            property real pendingPosition: 0
            property bool dragged: false

            onPressed: mouse => {
                pressY = mapToItem(root, mouse.x, mouse.y).y;
                startPosition = root.backend.subtitlePosition;
                pendingPosition = startPosition;
                dragged = false;
            }
            onPositionChanged: mouse => {
                if (!pressed)
                    return;
                const delta = mapToItem(root, mouse.x, mouse.y).y - pressY;
                if (!dragged && Math.abs(delta) < 8)
                    return;
                dragged = true;
                const minimum = subtitleDrag.height * 1.35 / root.height * 100;
                pendingPosition = Math.max(minimum, Math.min(100, startPosition + delta / root.height * 100));
                root.backend.previewSubtitlePosition(pendingPosition);
            }
            onReleased: {
                if (dragged) {
                    root.backend.previewSubtitlePosition(pendingPosition);
                    root.backend.commitSubtitlePosition();
                }
            }
            onCanceled: {
                if (dragged)
                    root.backend.cancelSubtitlePosition(startPosition);
            }
            onClicked: {
                if (!dragged)
                    root.backend.togglePause();
            }
        }
    }

    Timer {
        id: hideTimer
        interval: 2200
        running: root.transportVisible && root.backend.state === Playback.Playing && !root.settingsOpen && !openMenu.opened && !playbackMenu.opened && !topHover.hovered && !bottomHover.hovered
        onTriggered: root.transportVisible = false
    }

    Column {
        anchors.centerIn: parent
        spacing: 18
        visible: root.backend.state === Playback.Stopped && !root.backend.mediaLoaded

        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            text: root.russian ? qsTr("Откройте видео") : qsTr("Open a video")
            color: "#edf3fa"
            font.pixelSize: 25
            font.weight: Font.Medium
        }
        Button {
            anchors.horizontalCenter: parent.horizontalCenter
            text: root.russian ? qsTr("Открыть файл") : qsTr("Open file")
            background: Rectangle {
                radius: 9
                color: "#009df7"
            }
            onClicked: root.openFileRequested()
        }
    }

    Column {
        anchors.centerIn: parent
        spacing: 12
        visible: root.backend.state === Playback.Starting || root.backend.state === Playback.Loading

        BusyIndicator {
            anchors.horizontalCenter: parent.horizontalCenter
            running: parent.visible
        }

        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            visible: root.backend.onlineWaiting && root.backend.onlineProgress >= 0
            text: (root.russian ? qsTr("Подготовка YouTube %1%") : qsTr("Preparing YouTube %1%"))
                  .arg(root.backend.onlineProgress)
            color: "#edf3fa"
        }
    }

    Column {
        anchors.centerIn: parent
        width: Math.min(parent.width - 48, 600)
        spacing: 18
        visible: root.backend.errorText.length > 0

        Label {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            color: "#ffffff"
            text: root.backend.errorText
        }
        Button {
            anchors.horizontalCenter: parent.horizontalCenter
            text: root.russian ? qsTr("Открыть другой файл") : qsTr("Open another file")
            onClicked: root.openFileRequested()
        }
    }

    Item {
        id: topOverlay
        anchors.top: parent.top
        width: parent.width
        height: 156
        opacity: root.controlsShown ? 1 : 0
        enabled: opacity > 0.01
        Behavior on opacity {
            NumberAnimation {
                duration: 180
                easing.type: Easing.OutCubic
            }
        }

        Rectangle {
            anchors.fill: parent
            gradient: Gradient {
                GradientStop {
                    position: 0
                    color: "#e0000000"
                }
                GradientStop {
                    position: 1
                    color: "#00000000"
                }
            }
        }

        HoverHandler {
            id: topHover
        }

        RowLayout {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.leftMargin: 26
            anchors.rightMargin: 20
            anchors.topMargin: 20
            spacing: 4

            Label {
                Layout.fillWidth: true
                text: root.backend.title.length ? root.backend.title : "qplay"
                elide: Text.ElideRight
                color: "#ffffff"
                font.pixelSize: 22
                font.weight: Font.DemiBold
            }
            PlayerIconButton {
                objectName: "openMenuButton"
                iconSource: root.iconRoot + "folder-open.svg"
                hint: root.russian ? qsTr("Открыть") : qsTr("Open")
                onClicked: openMenu.open()
            }
        }
    }

    Item {
        id: bottomOverlay
        anchors.bottom: parent.bottom
        width: parent.width
        height: 205
        opacity: root.controlsShown ? 1 : 0
        enabled: opacity > 0.01
        Behavior on opacity {
            NumberAnimation {
                duration: 180
                easing.type: Easing.OutCubic
            }
        }

        Rectangle {
            anchors.fill: parent
            gradient: Gradient {
                GradientStop {
                    position: 0
                    color: "#00000000"
                }
                GradientStop {
                    position: 1
                    color: "#e6000000"
                }
            }
        }

        MouseArea {
            anchors.fill: parent
        }

        HoverHandler {
            id: bottomHover
        }

        ColumnLayout {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.leftMargin: 26
            anchors.rightMargin: 26
            anchors.bottomMargin: 18
            spacing: 7

            RowLayout {
                Layout.fillWidth: true
                spacing: 12

                Label {
                    text: root.clock(root.positionSeconds)
                    color: "#ffffff"
                    font.pixelSize: 15
                    font.weight: Font.DemiBold
                }

                Slider {
                    id: seekSlider
                    objectName: "seekSlider"
                    Layout.fillWidth: true
                    implicitHeight: 44
                    enabled: root.backend.mediaLoaded
                    from: 0
                    to: Math.max(root.backend.duration, 1)
                    value: root.backend.position
                    live: false
                    Accessible.name: root.russian ? qsTr("Позиция") : qsTr("Position")
                    onPressedChanged: {
                        if (!pressed)
                            root.backend.seek(value);
                    }
                    background: Item {
                        x: seekSlider.leftPadding
                        y: seekSlider.topPadding + seekSlider.availableHeight / 2 - 3
                        width: seekSlider.availableWidth
                        height: 6

                        Rectangle {
                            anchors.fill: parent
                            radius: 3
                            color: "#8cffffff"
                        }
                        Rectangle {
                            width: seekSlider.visualPosition * parent.width
                            height: parent.height
                            radius: 3
                            color: "#009df7"
                        }
                    }
                    handle: Rectangle {
                        x: seekSlider.leftPadding + seekSlider.visualPosition * (seekSlider.availableWidth - width)
                        y: seekSlider.topPadding + seekSlider.availableHeight / 2 - height / 2
                        width: 14
                        height: 14
                        radius: 7
                        color: "#ffffff"
                    }
                }

                Label {
                    text: root.clock(root.durationSeconds)
                    color: "#ffffff"
                    font.pixelSize: 15
                    font.weight: Font.DemiBold
                }
            }

            Item {
                Layout.fillWidth: true
                implicitHeight: 52

                RowLayout {
                    anchors.left: parent.left
                    anchors.verticalCenter: parent.verticalCenter
                    visible: root.backend.downloadState !== "unavailable"
                    spacing: 4

                    PlayerIconButton {
                        objectName: "downloadButton"
                        iconSource: root.iconRoot + (root.backend.downloadState === "queued" ? "check.svg" : "download.svg")
                        enabled: root.backend.downloadState === "ready" || root.backend.downloadState === "failed"
                        hint: root.backend.downloadState === "pending"
                              ? (root.russian ? qsTr("Добавление в загрузки") : qsTr("Adding to downloads"))
                              : root.backend.downloadState === "queued"
                              ? (root.russian ? qsTr("В загрузках") : qsTr("In downloads"))
                              : root.backend.downloadState === "failed"
                              ? (root.russian ? qsTr("Повторить загрузку") : qsTr("Retry download"))
                              : (root.russian ? qsTr("Сохранить видео") : qsTr("Save video"))
                        onClicked: root.backend.requestDownload()
                    }
                    Label {
                        visible: root.backend.downloadState === "pending" || root.backend.downloadState === "failed"
                        text: root.backend.downloadState === "pending"
                              ? (root.russian ? qsTr("Добавление…") : qsTr("Adding…"))
                              : (root.russian ? qsTr("Не удалось") : qsTr("Failed"))
                        color: root.backend.downloadState === "failed" ? "#ff9a9a" : "#b9bec4"
                        font.pixelSize: 13
                    }
                }

                RowLayout {
                    anchors.centerIn: parent
                    spacing: 4

                    PlayerIconButton {
                        iconSource: root.iconRoot + "rewind-backward-10.svg"
                        enabled: root.backend.mediaLoaded
                        hint: root.russian ? qsTr("Назад на 10 секунд") : qsTr("Back 10 seconds")
                        onClicked: root.backend.seek(Math.max(0, root.backend.position - 10))
                    }
                    PlayerIconButton {
                        id: pauseButton
                        objectName: "pauseButton"
                        implicitWidth: 52
                        implicitHeight: 52
                        iconSize: 28
                        iconSource: root.iconRoot + (root.backend.paused ? "player-play.svg" : "player-pause.svg")
                        enabled: root.backend.mediaLoaded
                        hint: root.backend.paused ? (root.russian ? qsTr("Продолжить") : qsTr("Play")) : (root.russian ? qsTr("Пауза") : qsTr("Pause"))
                        background: Rectangle {
                            color: pauseButton.hovered ? "#22ffffff" : "transparent"
                            border.color: pauseButton.visualFocus ? "#009df7" : "transparent"
                            border.width: 2
                            radius: 7
                        }
                        onClicked: root.backend.togglePause()
                    }
                    PlayerIconButton {
                        iconSource: root.iconRoot + "rewind-forward-10.svg"
                        enabled: root.backend.mediaLoaded
                        hint: root.russian ? qsTr("Вперёд на 10 секунд") : qsTr("Forward 10 seconds")
                        onClicked: root.backend.seek(root.backend.position + 10)
                    }
                }

                RowLayout {
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 4

                    PlayerIconButton {
                        objectName: "muteButton"
                        iconSource: root.iconRoot + (root.backend.muted ? "volume-off.svg" : "volume.svg")
                        enabled: root.backend.engineReady
                        hint: root.backend.muted ? (root.russian ? qsTr("Включить звук") : qsTr("Unmute")) : (root.russian ? qsTr("Без звука") : qsTr("Mute"))
                        onClicked: root.backend.changeMute(!root.backend.muted)
                    }

                    Slider {
                        id: volumeSlider
                        objectName: "volumeSlider"
                        Layout.preferredWidth: 96
                        implicitHeight: 44
                        from: 0
                        to: 100
                        value: root.backend.volume
                        enabled: root.backend.engineReady
                        Accessible.name: root.russian ? qsTr("Громкость") : qsTr("Volume")
                        onMoved: root.backend.changeVolume(value)
                        background: Item {
                            x: volumeSlider.leftPadding
                            y: volumeSlider.topPadding + volumeSlider.availableHeight / 2 - 2
                            width: volumeSlider.availableWidth
                            height: 4

                            Rectangle {
                                anchors.fill: parent
                                radius: 2
                                color: "#81ffffff"
                            }
                            Rectangle {
                                width: volumeSlider.visualPosition * parent.width
                                height: parent.height
                                radius: 2
                                color: "#ffffff"
                            }
                        }
                        handle: Rectangle {
                            x: volumeSlider.leftPadding + volumeSlider.visualPosition * (volumeSlider.availableWidth - width)
                            y: volumeSlider.topPadding + volumeSlider.availableHeight / 2 - height / 2
                            width: 10
                            height: 10
                            radius: 5
                            color: "#ffffff"
                        }
                    }

                    PlayerIconButton {
                        objectName: "settingsButton"
                        iconSource: root.iconRoot + "settings.svg"
                        hint: root.russian ? qsTr("Параметры воспроизведения") : qsTr("Playback settings")
                        onClicked: playbackMenu.open()
                    }
                    PlayerIconButton {
                        objectName: "fullscreenButton"
                        iconSource: root.iconRoot + "maximize.svg"
                        hint: root.russian ? qsTr("Полный экран") : qsTr("Fullscreen")
                        onClicked: root.fullscreenRequested()
                    }
                }
            }
        }
    }

    Popup {
        id: openMenu
        objectName: "openMenu"
        parent: Overlay.overlay
        x: parent.width - width - 66
        y: 74
        width: 248
        padding: 8
        modal: false
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        background: Rectangle {
            radius: 12
            color: "#d83d3f42"
            border.color: "#80ffffff"
        }

        contentItem: Column {
            spacing: 2

            ItemDelegate {
                id: fileItem
                width: parent.width
                implicitHeight: 44
                Accessible.name: root.russian ? qsTr("Открыть файл") : qsTr("Open file")
                onClicked: {
                    openMenu.close();
                    root.openFileRequested();
                }
                background: Rectangle {
                    radius: 7
                    color: fileItem.hovered ? "#343638" : "transparent"
                }
                contentItem: RowLayout {
                    spacing: 12
                    Image {
                        source: root.iconRoot + "folder-open.svg"
                        sourceSize: Qt.size(20, 20)
                        Layout.preferredWidth: 20
                        Layout.preferredHeight: 20
                    }
                    Label {
                        Layout.fillWidth: true
                        text: root.russian ? qsTr("Открыть файл") : qsTr("Open file")
                        color: "#ffffff"
                    }
                    Label {
                        text: "Ctrl+O"
                        color: "#a9bacb"
                        font.pixelSize: 12
                    }
                }
            }

            ItemDelegate {
                id: linkItem
                width: parent.width
                implicitHeight: 44
                Accessible.name: root.russian ? qsTr("Открыть ссылку") : qsTr("Open link")
                onClicked: {
                    openMenu.close();
                    root.openLinkRequested();
                }
                background: Rectangle {
                    radius: 7
                    color: linkItem.hovered ? "#343638" : "transparent"
                }
                contentItem: RowLayout {
                    spacing: 12
                    Image {
                        source: root.iconRoot + "link.svg"
                        sourceSize: Qt.size(20, 20)
                        Layout.preferredWidth: 20
                        Layout.preferredHeight: 20
                    }
                    Label {
                        Layout.fillWidth: true
                        text: root.russian ? qsTr("Открыть ссылку") : qsTr("Open link")
                        color: "#ffffff"
                    }
                    Label {
                        text: "Ctrl+L"
                        color: "#a9bacb"
                        font.pixelSize: 12
                    }
                }
            }
        }
    }

    PlaybackMenu {
        id: playbackMenu
        backend: root.backend
        host: root
        russian: root.russian
        onAdvancedRequested: root.settingsRequested()
    }

    Shortcut {
        sequence: "Space"
        enabled: root.visible && !root.settingsOpen && !openMenu.opened && !playbackMenu.opened && root.backend.mediaLoaded
        onActivated: root.backend.togglePause()
    }
    Shortcut {
        sequence: "Right"
        enabled: root.visible && !root.settingsOpen && !openMenu.opened && !playbackMenu.opened && root.backend.mediaLoaded
        onActivated: root.backend.seek(root.backend.position + 5)
    }
    Shortcut {
        sequence: "Left"
        enabled: root.visible && !root.settingsOpen && !openMenu.opened && !playbackMenu.opened && root.backend.mediaLoaded
        onActivated: root.backend.seek(Math.max(0, root.backend.position - 5))
    }
    Shortcut {
        sequence: "F"
        enabled: !root.settingsOpen && !openMenu.opened && !playbackMenu.opened
        onActivated: root.fullscreenRequested()
    }
}
