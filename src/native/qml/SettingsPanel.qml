pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Qplay.Player

Popup {
    id: root
    objectName: "settingsPanel"
    required property PlayerController backend
    required property Item host
    property bool russian: false
    property bool dark: true
    readonly property string iconRoot: "qrc:/qt/qml/Qplay/Views/assets/icons/"

    readonly property color surface: dark ? "#191d22" : "#f8f9fb"
    readonly property color textColor: dark ? "#f2f5f9" : "#18232c"
    readonly property color mutedColor: dark ? "#aab7c3" : "#4a5a66"
    readonly property color optionColor: dark ? "#292f36" : "#e7ecf0"

    parent: host
    x: host.width - width
    width: Math.min(365, host.width)
    height: host.height
    padding: 22
    modal: true
    dim: true
    focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    onOpened: root.backend.loadSubtitleModels()
    enter: Transition {
        NumberAnimation { property: "x"; from: root.host.width; to: root.host.width - root.width; duration: 180; easing.type: Easing.OutCubic }
    }
    exit: Transition {
        NumberAnimation { property: "x"; to: root.host.width; duration: 135; easing.type: Easing.InCubic }
    }
    Overlay.modal: Rectangle {
        color: "#48000000"
        Behavior on opacity { NumberAnimation { duration: 140 } }
    }
    background: Rectangle { color: root.surface }

    component Caption: Label {
        Layout.topMargin: 15
        Layout.bottomMargin: 3
        color: root.textColor
        font.pixelSize: 14
        font.weight: Font.DemiBold
    }

    component Choices: RowLayout {
        id: choices
        required property var items
        required property string current
        signal picked(string value)
        Layout.fillWidth: true
        spacing: 3
        Repeater {
            model: choices.items
            delegate: Button {
                id: choice
                required property var modelData
                objectName: choices.objectName + modelData.value
                Layout.fillWidth: true
                Layout.preferredWidth: 1
                implicitHeight: 34
                text: modelData.label
                checked: choices.current === modelData.value
                palette.buttonText: root.textColor
                palette.brightText: root.dark ? "#94d6ff" : "#005c92"
                background: Rectangle {
                    radius: 5
                    color: choice.checked ? (root.dark ? "#17394e" : "#d5edfb")
                           : choice.hovered || choice.visualFocus ? (root.dark ? "#38424c" : "#dce3e9") : root.optionColor
                }
                onClicked: choices.picked(modelData.value)
            }
        }
    }

    contentItem: ScrollView {
        id: settingsScroll
        objectName: "settingsScroll"
        clip: true
        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

        ColumnLayout {
            id: settingsBody
            width: settingsScroll.availableWidth
            spacing: 0

            RowLayout {
                Layout.fillWidth: true

                Label {
                    Layout.fillWidth: true
                    text: root.russian ? qsTr("Настройки") : qsTr("Settings")
                    color: root.textColor
                    font.pixelSize: 19
                    font.weight: Font.DemiBold
                }
                PlayerIconButton {
                    iconSource: root.iconRoot + "x.svg"
                    foreground: root.textColor
                    iconSize: 20
                    hoverColor: root.dark ? "#2cffffff" : "#16000000"
                    hint: root.russian ? qsTr("Закрыть настройки") : qsTr("Close settings")
                    onClicked: root.close()
                }
            }

            Caption { text: root.russian ? qsTr("Оформление") : qsTr("Appearance") }
            Choices {
                objectName: "appearance"
                items: [
                    { value: "system", label: root.russian ? qsTr("Система") : qsTr("System") },
                    { value: "light", label: root.russian ? qsTr("Светлая") : qsTr("Light") },
                    { value: "dark", label: root.russian ? qsTr("Тёмная") : qsTr("Dark") }
                ]
                current: root.backend.appearance
                onPicked: value => root.backend.changeAppearance(value)
            }
            Caption { text: root.russian ? qsTr("Язык") : qsTr("Language") }
            Choices {
                objectName: "language"
                items: [{ value: "ru", label: "Русский" }, { value: "en", label: "English" }]
                current: root.backend.language
                onPicked: value => root.backend.changeLanguage(value)
            }
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 12
                spacing: 2
                Label {
                    Layout.fillWidth: true
                    text: root.russian ? qsTr("Задержка звука") : qsTr("Audio delay")
                    color: root.textColor
                }
                PlayerIconButton {
                    iconSource: root.iconRoot + "minus.svg"
                    foreground: root.textColor
                    implicitWidth: 31
                    iconSize: 17
                    hint: root.russian ? qsTr("Уменьшить задержку") : qsTr("Decrease delay")
                    onClicked: root.backend.changeAudioDelay(Math.round((root.backend.audioDelay - 0.05) * 100) / 100)
                }
                Label {
                    Layout.minimumWidth: 55
                    horizontalAlignment: Text.AlignHCenter
                    text: root.backend.audioDelay.toFixed(2) + (root.russian ? " с" : " s")
                    color: root.textColor
                }
                PlayerIconButton {
                    iconSource: root.iconRoot + "plus.svg"
                    foreground: root.textColor
                    implicitWidth: 31
                    iconSize: 17
                    hint: root.russian ? qsTr("Увеличить задержку") : qsTr("Increase delay")
                    onClicked: root.backend.changeAudioDelay(Math.round((root.backend.audioDelay + 0.05) * 100) / 100)
                }
                PlayerIconButton {
                    objectName: "resetDelay"
                    iconSource: root.iconRoot + "reset.svg"
                    foreground: root.textColor
                    implicitWidth: 31
                    iconSize: 17
                    enabled: root.backend.audioDelay !== 0
                    hint: root.russian ? qsTr("Сбросить задержку") : qsTr("Reset delay")
                    onClicked: root.backend.changeAudioDelay(0)
                }
            }
            Caption { text: root.russian ? qsTr("Перевод субтитров") : qsTr("Subtitle translation") }
            Switch {
                id: translationSwitch
                objectName: "subtitleTranslationSwitch"
                Layout.fillWidth: true
                Layout.topMargin: 7
                implicitHeight: 36
                text: root.russian ? qsTr("Включить для всех видео") : qsTr("Enable for all videos")
                checked: root.backend.subtitleTranslationEnabled
                indicator: Rectangle {
                    implicitWidth: 44
                    implicitHeight: 26
                    y: (translationSwitch.height - height) / 2
                    radius: 13
                    color: translationSwitch.checked ? "#009df7" : (root.dark ? "#5b6065" : "#b7c2ce")
                    border.color: translationSwitch.visualFocus ? "#009df7" : "transparent"
                    border.width: 2

                    Rectangle {
                        x: translationSwitch.checked ? parent.width - width - 3 : 3
                        y: 3
                        width: 20
                        height: 20
                        radius: 10
                        color: "#ffffff"
                    }
                }
                contentItem: Label {
                    leftPadding: translationSwitch.indicator.width + 10
                    text: translationSwitch.text
                    color: root.textColor
                    verticalAlignment: Text.AlignVCenter
                }
                onToggled: root.backend.changeSubtitleTranslation(checked)
            }
            Choices {
                objectName: "subtitleTarget"
                items: [{ value: "ru", label: "Русский" }, { value: "en", label: "English" }]
                current: root.backend.subtitleTarget
                onPicked: value => root.backend.changeSubtitleTarget(value)
            }
            Label {
                Layout.topMargin: 10
                text: root.russian ? qsTr("Модель") : qsTr("Model")
                color: root.textColor
                font.pixelSize: 15
            }
            SubtitleChoice {
                objectName: "subtitleModel"
                Layout.fillWidth: true
                Layout.topMargin: 5
                choices: root.backend.subtitleModels
                selectedValue: root.backend.subtitleModel
                russian: root.russian
                dark: root.dark
                textColor: root.textColor
                mutedColor: root.mutedColor
                optionColor: root.optionColor
                Accessible.name: root.russian ? qsTr("Модель перевода") : qsTr("Translation model")
                onChoiceActivated: (value) => root.backend.changeSubtitleModel(value)
            }
            Label {
                Layout.topMargin: 9
                text: root.russian ? qsTr("Глубина рассуждения") : qsTr("Effort")
                color: root.textColor
                font.pixelSize: 15
            }
            SubtitleChoice {
                objectName: "subtitleEffort"
                Layout.fillWidth: true
                Layout.topMargin: 5
                choices: root.backend.subtitleEfforts
                selectedValue: root.backend.subtitleEffort
                russian: root.russian
                dark: root.dark
                textColor: root.textColor
                mutedColor: root.mutedColor
                optionColor: root.optionColor
                Accessible.name: root.russian ? qsTr("Усилие модели") : qsTr("Model effort")
                onChoiceActivated: (value) => root.backend.changeSubtitleEffort(value)
            }
            Label {
                Layout.fillWidth: true
                Layout.topMargin: 7
                visible: ["unavailable", "modelUnavailable", "failed"].includes(root.backend.subtitleStatus)
                text: root.backend.subtitleStatus === "modelUnavailable"
                      ? (root.russian ? qsTr("Выбранная модель недоступна") : qsTr("Selected model is unavailable"))
                      : root.backend.subtitleStatus === "unavailable"
                        ? (root.russian ? qsTr("Codex недоступен") : qsTr("Codex is unavailable"))
                        : (root.russian ? qsTr("Не удалось перевести субтитры") : qsTr("Could not translate subtitles"))
                color: "#ed777e"
                wrapMode: Text.Wrap
            }

            Label {
                Layout.topMargin: 19
                text: root.russian ? qsTr("Обновления") : qsTr("Updates")
                color: root.mutedColor
                font.pixelSize: 14
                font.weight: Font.DemiBold
            }
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 8
                spacing: 8

                Button {
                    id: checkUpdate
                    objectName: "checkUpdate"
                    Layout.fillWidth: true
                    implicitHeight: 36
                    text: root.russian ? qsTr("Проверить") : qsTr("Check")
                    enabled: root.backend.updateState !== "checking" && root.backend.updateState !== "downloading"
                             && root.backend.updateState !== "ready" && root.backend.updateState !== "installing"
                    palette.buttonText: root.textColor
                    background: Rectangle {
                        radius: 7
                        color: checkUpdate.hovered ? (root.dark ? "#425365" : "#dfe7f0") : root.optionColor
                        border.color: checkUpdate.visualFocus ? "#009df7" : "transparent"
                        border.width: 2
                    }
                    onClicked: root.backend.checkUpdates()
                }
                Button {
                    id: applyUpdate
                    objectName: "applyUpdate"
                    Layout.fillWidth: true
                    implicitHeight: 36
                    visible: root.backend.updateVersion.length > 0
                    enabled: root.backend.updateState !== "downloading" && root.backend.updateState !== "installing"
                    text: {
                        if (root.backend.portable)
                            return root.russian ? qsTr("Скачать ZIP") : qsTr("Download ZIP")
                        if (root.backend.updateState === "ready")
                            return root.russian ? qsTr("Установить") : qsTr("Install")
                        if (root.backend.updateState === "installFailed" || root.backend.updateState === "downloadFailed")
                            return root.russian ? qsTr("Повторить") : qsTr("Retry")
                        return root.russian ? qsTr("Обновить") : qsTr("Update")
                    }
                    palette.buttonText: "#ffffff"
                    background: Rectangle {
                        radius: 7
                        color: applyUpdate.hovered ? "#0089db" : "#009df7"
                        border.color: applyUpdate.visualFocus ? "#ffffff" : "transparent"
                        border.width: 2
                    }
                    onClicked: {
                        if (root.backend.portable) {
                            Qt.openUrlExternally(root.backend.updateUrl)
                        } else if (root.backend.updateState === "ready") {
                            if (root.backend.installUpdate())
                                Qt.quit()
                        } else {
                            root.backend.downloadUpdate()
                        }
                    }
                }
            }
            Label {
                id: updateStatus
                objectName: "updateStatus"
                Layout.fillWidth: true
                Layout.topMargin: 7
                visible: root.backend.updateState !== "idle"
                color: root.backend.updateError.length > 0 ? "#ed777e" : root.mutedColor
                font.pixelSize: 13
                wrapMode: Text.Wrap
                Accessible.description: root.backend.updateError
                text: {
                    switch (root.backend.updateState) {
                    case "checking":
                        return root.russian ? qsTr("Проверка обновлений…") : qsTr("Checking for updates…")
                    case "unpublished":
                        return root.russian ? qsTr("Пока нет опубликованной версии") : qsTr("No release is available yet")
                    case "current":
                        return root.russian ? qsTr("Установлена актуальная версия") : qsTr("qplay is up to date")
                    case "available":
                    case "downloadFailed":
                        if (root.backend.updateState === "downloadFailed")
                            return root.russian ? qsTr("Не удалось загрузить обновление") : qsTr("Couldn't download the update")
                        return root.russian ? qsTr("Доступна версия %1").arg(root.backend.updateVersion)
                                            : qsTr("Version %1 is available").arg(root.backend.updateVersion)
                    case "downloading":
                        return root.russian ? qsTr("Загрузка: %1%").arg(root.backend.updateProgress)
                                            : qsTr("Downloading: %1%").arg(root.backend.updateProgress)
                    case "ready":
                        return root.russian ? qsTr("Обновление установится после закрытия всех окон qplay") : qsTr("The update will install when all qplay windows close")
                    case "checkFailed":
                        return root.russian ? qsTr("Не удалось проверить обновления") : qsTr("Couldn't check for updates")
                    case "installFailed":
                        return root.russian ? qsTr("Не удалось начать установку") : qsTr("Couldn't start installation")
                    default:
                        return ""
                    }
                }
                ToolTip.visible: updateHover.hovered && root.backend.updateError.length > 0
                ToolTip.text: root.backend.updateError
                HoverHandler { id: updateHover }
            }

            Label {
                Layout.fillWidth: true
                Layout.topMargin: 14
                visible: root.backend.settingsError.length > 0
                text: root.backend.settingsError
                color: "#ed777e"
                wrapMode: Text.Wrap
            }
        }
    }
}
