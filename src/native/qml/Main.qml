import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import Qplay.Player

ApplicationWindow {
    id: root
    required property PlayerController backend
    readonly property bool russian: backend.language === "ru"
    readonly property bool dark: backend.appearance === "dark" || (backend.appearance === "system" && Application.styleHints.colorScheme === Qt.Dark)

    visible: true
    width: 1280
    height: 720
    minimumWidth: 800
    minimumHeight: 500
    title: backend.title.length ? backend.title + " - qplay" : "qplay"
    color: "#202123"

    palette.window: dark ? "#1b2028" : "#f8fafc"
    palette.windowText: dark ? "#f5f7fa" : "#1a2635"
    palette.button: dark ? "#303842" : "#e8edf3"
    palette.buttonText: dark ? "#f5f7fa" : "#1a2635"
    palette.base: dark ? "#252c35" : "#ffffff"
    palette.text: dark ? "#f5f7fa" : "#1a2635"
    palette.highlight: "#009df7"
    palette.highlightedText: "#ffffff"

    PlayerScene {
        anchors.fill: parent
        backend: root.backend
        russian: root.russian
        settingsOpen: settingsPanel.opened
        onOpenFileRequested: fileDialog.open()
        onOpenLinkRequested: linkDialog.open()
        onSettingsRequested: settingsPanel.open()
        onFullscreenRequested: root.visibility = root.visibility === Window.FullScreen ? Window.Windowed : Window.FullScreen
    }

    SettingsPanel {
        id: settingsPanel
        backend: root.backend
        host: root.contentItem
        russian: root.russian
        dark: root.dark
    }

    FileDialog {
        id: fileDialog
        title: root.russian ? qsTr("Открыть видео") : qsTr("Open video")
        onAccepted: root.backend.openMedia(selectedFile.toString())
    }

    Dialog {
        id: linkDialog
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: Math.min(480, root.width - 32)
        title: root.russian ? qsTr("Открыть ссылку") : qsTr("Open link")
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        onOpened: linkField.forceActiveFocus()
        onAccepted: root.backend.openMedia(linkField.text.trim())

        contentItem: TextField {
            id: linkField
            objectName: "linkField"
            placeholderText: "https://"
            inputMethodHints: Qt.ImhUrlCharactersOnly | Qt.ImhNoAutoUppercase
            selectByMouse: true
            onAccepted: linkDialog.accept()
        }
    }

    Shortcut {
        sequence: "Ctrl+O"
        onActivated: fileDialog.open()
    }
    Shortcut {
        sequence: "Ctrl+L"
        onActivated: linkDialog.open()
    }
}
