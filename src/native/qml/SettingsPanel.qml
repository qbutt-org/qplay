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

    parent: host
    x: parent.width - width - 22
    y: 76
    width: Math.min(344, parent.width - 32)
    padding: 20
    modal: true
    dim: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    Overlay.modal: Rectangle {
        color: "#59000000"
    }
    background: Rectangle {
        radius: 14
        color: root.surface
        border.color: root.dark ? "#80ffffff" : "#dddfe2"
    }

    contentItem: ColumnLayout {
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
            Layout.fillWidth: true
            Layout.topMargin: 14
            visible: root.backend.settingsError.length > 0
            text: root.backend.settingsError
            color: "#ed777e"
            wrapMode: Text.Wrap
        }
    }
}
