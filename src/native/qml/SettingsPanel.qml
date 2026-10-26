pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
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

    readonly property color surface: dark ? "#e33d3f42" : "#e9f4f5f6"
    readonly property color textColor: dark ? "#f2f6fb" : "#192433"
    readonly property color mutedColor: dark ? "#c1cedc" : "#35465a"
    readonly property color optionColor: dark ? "#343638" : "#e9eef4"
    readonly property int topOffset: 76

    parent: host
    x: root.host ? root.host.width - width - 22 : 0
    y: root.topOffset
    width: root.host ? Math.min(344, root.host.width - 32) : 0
    height: root.host ? Math.min(settingsBody.implicitHeight + topPadding + bottomPadding, root.host.height - root.topOffset - 20) : 0
    padding: 20
    modal: true
    dim: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    onOpened: root.backend.loadSubtitleModels()

    Overlay.modal: Rectangle {
        color: "#59000000"
    }
    background: Rectangle {
        radius: 14
        color: root.surface
        border.color: root.dark ? "#80ffffff" : "#dddfe2"
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
                    iconSource: root.dark ? root.iconRoot + "x.svg" : root.iconRoot + "x-dark.svg"
                    iconSize: 20
                    hoverColor: root.dark ? "#2cffffff" : "#16000000"
                    hint: root.russian ? qsTr("Закрыть настройки") : qsTr("Close settings")
                    onClicked: root.close()
                }
            }

            Label {
                Layout.topMargin: 17
                text: root.russian ? qsTr("Оформление") : qsTr("Appearance")
                color: root.mutedColor
                font.pixelSize: 14
                font.weight: Font.DemiBold
            }
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 8
                spacing: 6

                Repeater {
                    model: [
                        {
                            value: "system",
                            label: root.russian ? qsTr("Система") : qsTr("System")
                        },
                        {
                            value: "light",
                            label: root.russian ? qsTr("Светлая") : qsTr("Light")
                        },
                        {
                            value: "dark",
                            label: root.russian ? qsTr("Тёмная") : qsTr("Dark")
                        }
                    ]
                    delegate: Button {
                        id: option
                        required property var modelData
                        Layout.fillWidth: true
                        implicitHeight: 44
                        text: modelData.label
                        font.pixelSize: 14
                        checked: root.backend.appearance === modelData.value
                        contentItem: Label {
                            text: option.text
                            color: option.checked ? "#ffffff" : root.textColor
                            font: option.font
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                        background: Rectangle {
                            radius: 7
                            color: option.checked ? "#009df7" : root.optionColor
                            border.color: option.visualFocus ? "#009df7" : "transparent"
                            border.width: 2
                        }
                        onClicked: root.backend.changeAppearance(modelData.value)
                    }
                }
            }

            Label {
                Layout.topMargin: 19
                text: root.russian ? qsTr("Аудио и видео") : qsTr("Audio and video")
                color: root.mutedColor
                font.pixelSize: 14
                font.weight: Font.DemiBold
            }
            Label {
                Layout.topMargin: 9
                text: root.russian ? qsTr("Задержка звука") : qsTr("Audio delay")
                color: root.textColor
                font.pixelSize: 15
            }
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 5
                spacing: 8

                Button {
                    id: decreaseDelay
                    implicitWidth: 44
                    implicitHeight: 44
                    Accessible.name: root.russian ? qsTr("Уменьшить задержку звука") : qsTr("Decrease audio delay")
                    contentItem: Item {
                        Image {
                            anchors.centerIn: parent
                            width: 20
                            height: 20
                            source: root.iconRoot + (root.dark ? "minus.svg" : "minus-dark.svg")
                            sourceSize: Qt.size(20, 20)
                        }
                    }
                    background: Rectangle {
                        radius: 7
                        color: decreaseDelay.hovered ? (root.dark ? "#425365" : "#dfe7f0") : root.optionColor
                        border.color: decreaseDelay.visualFocus ? "#009df7" : "transparent"
                        border.width: 2
                    }
                    onClicked: root.backend.changeAudioDelay(Math.round((root.backend.audioDelay - 0.05) * 100) / 100)
                }
                Label {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    text: (root.backend.audioDelay >= 0 ? "+" : "") + root.backend.audioDelay.toFixed(2) + " s"
                    color: root.textColor
                    font.pixelSize: 15
                    Accessible.name: root.russian ? qsTr("Задержка звука %1 секунды").arg(root.backend.audioDelay.toFixed(2)) : qsTr("Audio delay %1 seconds").arg(root.backend.audioDelay.toFixed(2))
                }
                Button {
                    id: increaseDelay
                    implicitWidth: 44
                    implicitHeight: 44
                    Accessible.name: root.russian ? qsTr("Увеличить задержку звука") : qsTr("Increase audio delay")
                    contentItem: Item {
                        Image {
                            anchors.centerIn: parent
                            width: 20
                            height: 20
                            source: root.iconRoot + (root.dark ? "plus.svg" : "plus-dark.svg")
                            sourceSize: Qt.size(20, 20)
                        }
                    }
                    background: Rectangle {
                        radius: 7
                        color: increaseDelay.hovered ? (root.dark ? "#425365" : "#dfe7f0") : root.optionColor
                        border.color: increaseDelay.visualFocus ? "#009df7" : "transparent"
                        border.width: 2
                    }
                    onClicked: root.backend.changeAudioDelay(Math.round((root.backend.audioDelay + 0.05) * 100) / 100)
                }
                Button {
                    id: resetDelay
                    implicitHeight: 44
                    text: root.russian ? qsTr("Сброс") : qsTr("Reset")
                    font.pixelSize: 13
                    enabled: root.backend.audioDelay !== 0
                    palette.buttonText: root.textColor
                    background: Rectangle {
                        radius: 7
                        color: resetDelay.hovered ? (root.dark ? "#425365" : "#dfe7f0") : root.optionColor
                        border.color: resetDelay.visualFocus ? "#009df7" : "transparent"
                        border.width: 2
                    }
                    onClicked: root.backend.changeAudioDelay(0)
                }
            }
            Label {
                Layout.fillWidth: true
                Layout.topMargin: 6
                text: root.russian ? qsTr("Плюс задерживает звук") : qsTr("Positive delays audio")
                color: root.mutedColor
                font.pixelSize: 12
            }

            Label {
                Layout.topMargin: 17
                text: root.russian ? qsTr("Язык") : qsTr("Language")
                color: root.mutedColor
                font.pixelSize: 14
                font.weight: Font.DemiBold
            }
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 8
                spacing: 6

                Repeater {
                    model: [
                        {
                            value: "ru",
                            label: "Русский"
                        },
                        {
                            value: "en",
                            label: "English"
                        }
                    ]
                    delegate: Button {
                        id: languageOption
                        required property var modelData
                        Layout.fillWidth: true
                        implicitHeight: 44
                        text: modelData.label
                        font.pixelSize: 14
                        checked: root.backend.language === modelData.value
                        contentItem: Label {
                            text: languageOption.text
                            color: languageOption.checked ? "#ffffff" : root.textColor
                            font: languageOption.font
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                        background: Rectangle {
                            radius: 7
                            color: languageOption.checked ? "#009df7" : root.optionColor
                            border.color: languageOption.visualFocus ? "#009df7" : "transparent"
                            border.width: 2
                        }
                        onClicked: root.backend.changeLanguage(modelData.value)
                    }
                }
            }

            Label {
                Layout.topMargin: 19
                text: root.russian ? qsTr("Перевод субтитров") : qsTr("Subtitle translation")
                color: root.mutedColor
                font.pixelSize: 14
                font.weight: Font.DemiBold
            }
            Switch {
                id: translationSwitch
                objectName: "subtitleTranslationSwitch"
                Layout.fillWidth: true
                Layout.topMargin: 7
                implicitHeight: 44
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
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 8
                spacing: 6

                Repeater {
                    model: [
                        { value: "ru", label: "Русский" },
                        { value: "en", label: "English" }
                    ]
                    delegate: Button {
                        id: targetOption
                        required property var modelData
                        objectName: "subtitleTarget" + modelData.value
                        Layout.fillWidth: true
                        implicitHeight: 44
                        text: modelData.label
                        checked: root.backend.subtitleTarget === modelData.value
                        contentItem: Label {
                            text: targetOption.text
                            color: targetOption.checked ? "#ffffff" : root.textColor
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                        background: Rectangle {
                            radius: 7
                            color: targetOption.checked ? "#009df7" : root.optionColor
                        }
                        onClicked: root.backend.changeSubtitleTarget(modelData.value)
                    }
                }
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
                text: root.russian ? qsTr("Усилие") : qsTr("Effort")
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
                    implicitHeight: 44
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
                    implicitHeight: 44
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
