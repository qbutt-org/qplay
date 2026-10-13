pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import Qplay.Player

Popup {
    id: root
    objectName: "playbackMenu"
    required property PlayerController backend
    required property Item host
    property bool russian: false
    property int page: 0
    readonly property string iconRoot: "qrc:/qt/qml/Qplay/Views/assets/icons/"
    readonly property string subtitleChoice: {
        if (backend.subtitleTranslationActive) {
            if (backend.subtitleTarget === "ru")
                return russian ? qsTr("Перевод: русский") : qsTr("Translation: Russian")
            return russian ? qsTr("Перевод: английский") : qsTr("Translation: English")
        }
        if (backend.subtitleSelection === "no")
            return russian ? qsTr("Выкл.") : qsTr("Off")
        if (backend.subtitleSelection === "auto")
            return russian ? qsTr("Авто") : qsTr("Auto")
        return (russian ? qsTr("Дорожка %1") : qsTr("Track %1")).arg(backend.subtitleSelection)
    }
    readonly property string audioChoice: backend.audioSelection === "auto" ? (russian ? qsTr("Авто") : qsTr("Auto")) : (russian ? qsTr("Дорожка %1") : qsTr("Track %1")).arg(backend.audioSelection)
    readonly property string fxStatus: backend.framegenStatus === "loading" ? (russian ? qsTr("Подготовка") : qsTr("Preparing")) : backend.framegenStatus === "active" ? (russian ? qsTr("Генерация кадров") : qsTr("Frame generation")) : backend.effectiveShader === "off" ? (russian ? qsTr("Выкл.") : qsTr("Off")) : backend.effectiveShader === "fsr" ? "FSR" : "Anime4K"

    signal advancedRequested

    parent: host
    x: parent.width - width - 26
    y: parent.height - height - 104
    width: Math.min(328, parent.width - 32)
    height: Math.min(460, menuBody.implicitHeight + 36, parent.height - 130)
    padding: 18
    modal: false
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
    onClosed: page = 0

    background: Rectangle {
        radius: 14
        color: "#d83d3f42"
        border.color: "#80ffffff"
    }

    contentItem: ScrollView {
        id: scroll
        clip: true
        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

        ColumnLayout {
            id: menuBody
            width: scroll.availableWidth
            spacing: 4

            RowLayout {
                Layout.fillWidth: true
                Layout.bottomMargin: 8
                spacing: 4

                PlayerIconButton {
                    visible: root.page !== 0
                    iconSource: root.iconRoot + "chevron-left.svg"
                    iconSize: 20
                    hint: root.russian ? qsTr("Назад") : qsTr("Back")
                    onClicked: root.page = 0
                }
                Label {
                    Layout.fillWidth: true
                    text: root.page === 0 ? (root.russian ? qsTr("Воспроизведение") : qsTr("Playback")) : root.page === 1 ? (root.russian ? qsTr("Аудиодорожка") : qsTr("Audio track")) : root.page === 2 ? (root.russian ? qsTr("Субтитры") : qsTr("Subtitles")) : root.page === 3 ? (root.russian ? qsTr("Скорость") : qsTr("Speed")) : "FX"
                    color: "#ffffff"
                    font.pixelSize: 19
                    font.weight: Font.DemiBold
                }
            }

            Repeater {
                model: root.page === 0 ? [
                    {
                        title: root.russian ? qsTr("Аудиодорожка") : qsTr("Audio track"),
                        value: root.audioChoice,
                        page: 1
                    },
                    {
                        title: root.russian ? qsTr("Субтитры") : qsTr("Subtitles"),
                        value: root.subtitleChoice,
                        page: 2
                    },
                    {
                        title: root.russian ? qsTr("Скорость") : qsTr("Speed"),
                        value: Math.round(root.backend.speed * 100) / 100 + "×",
                        page: 3
                    },
                    {
                        title: "FX",
                        value: root.fxStatus,
                        page: 4
                    }
                ] : []
                delegate: ItemDelegate {
                    id: mainRow
                    required property var modelData
                    objectName: modelData.page === 3 ? "speedMenuRow" : modelData.page === 4 ? "fxMenuRow" : ""
                    Layout.fillWidth: true
                    implicitHeight: 50
                    Accessible.name: modelData.title + ": " + modelData.value
                    background: Rectangle {
                        radius: 7
                        color: mainRow.hovered ? "#343638" : "transparent"
                        border.color: mainRow.visualFocus ? "#009df7" : "transparent"
                        border.width: 2
                    }
                    contentItem: RowLayout {
                        spacing: 8
                        Label {
                            Layout.fillWidth: true
                            text: mainRow.modelData.title
                            color: "#ffffff"
                            font.pixelSize: 15
                        }
                        Label {
                            text: mainRow.modelData.value
                            color: "#b9bec4"
                            font.pixelSize: 14
                        }
                        Image {
                            source: root.iconRoot + "chevron-right.svg"
                            sourceSize: Qt.size(16, 16)
                            Layout.preferredWidth: 16
                            Layout.preferredHeight: 16
                        }
                    }
                    onClicked: root.page = modelData.page
                }
            }

            ItemDelegate {
                id: advanced
                objectName: "advancedButton"
                visible: root.page === 0
                Layout.fillWidth: true
                Layout.topMargin: 10
                implicitHeight: 44
                text: root.russian ? qsTr("Дополнительные настройки") : qsTr("Advanced settings")
                Accessible.name: text
                background: Rectangle {
                    radius: 7
                    color: advanced.hovered ? "#343638" : "transparent"
                    border.color: advanced.visualFocus ? "#009df7" : "transparent"
                    border.width: 2
                }
                contentItem: RowLayout {
                    Label {
                        Layout.fillWidth: true
                        text: advanced.text
                        color: "#ffffff"
                        font.pixelSize: 15
                    }
                    Image {
                        source: root.iconRoot + "chevron-right.svg"
                        sourceSize: Qt.size(16, 16)
                        Layout.preferredWidth: 16
                        Layout.preferredHeight: 16
                    }
                }
                onClicked: {
                    root.close();
                    root.advancedRequested();
                }
            }

            ColumnLayout {
                visible: root.page === 1
                Layout.fillWidth: true
                spacing: 2

                ItemDelegate {
                    id: audioAuto
                    objectName: "audioAutoButton"
                    Layout.fillWidth: true
                    implicitHeight: 44
                    enabled: root.backend.mediaLoaded
                    text: root.russian ? qsTr("Авто") : qsTr("Auto")
                    Accessible.name: text
                    background: Rectangle {
                        radius: 7
                        color: audioAuto.hovered ? "#343638" : "transparent"
                        border.color: audioAuto.visualFocus ? "#009df7" : "transparent"
                        border.width: 2
                    }
                    contentItem: RowLayout {
                        Label {
                            Layout.fillWidth: true
                            text: audioAuto.text
                            color: "#ffffff"
                        }
                        Image {
                            visible: root.backend.audioSelection === "auto"
                            source: root.iconRoot + "check.svg"
                            sourceSize: Qt.size(20, 20)
                            Layout.preferredWidth: 20
                            Layout.preferredHeight: 20
                        }
                    }
                    onClicked: root.backend.selectAutomaticTrack("audio")
                }

                Repeater {
                    model: root.backend.audioTracks
                    delegate: ItemDelegate {
                        id: audioTrack
                        required property int trackId
                        required property string label
                        required property bool selected
                        Layout.fillWidth: true
                        implicitHeight: 44
                        text: (root.russian ? qsTr("Дорожка %1") : qsTr("Track %1")).arg(trackId) + (label.length ? " — " + label : "")
                        Accessible.name: text
                        background: Rectangle {
                            radius: 7
                            color: audioTrack.hovered ? "#343638" : "transparent"
                            border.color: audioTrack.visualFocus ? "#009df7" : "transparent"
                            border.width: 2
                        }
                        contentItem: RowLayout {
                            Label {
                                Layout.fillWidth: true
                                text: audioTrack.text
                                color: "#ffffff"
                                elide: Text.ElideRight
                            }
                            Image {
                                visible: audioTrack.selected && root.backend.audioSelection !== "auto"
                                source: root.iconRoot + "check.svg"
                                sourceSize: Qt.size(20, 20)
                                Layout.preferredWidth: 20
                                Layout.preferredHeight: 20
                            }
                        }
                        onClicked: root.backend.selectTrack("audio", trackId)
                    }
                }
            }

            ColumnLayout {
                visible: root.page === 2
                Layout.fillWidth: true
                spacing: 2

                ItemDelegate {
                    id: translateSubtitles
                    Layout.fillWidth: true
                    implicitHeight: 44
                    enabled: root.backend.mediaLoaded
                    text: root.backend.subtitleTarget === "ru"
                          ? (root.russian ? qsTr("Перевести на русский") : qsTr("Translate to Russian"))
                          : (root.russian ? qsTr("Перевести на английский") : qsTr("Translate to English"))
                    Accessible.name: text
                    contentItem: RowLayout {
                        Label {
                            Layout.fillWidth: true
                            text: translateSubtitles.text
                            color: "#62beff"
                        }
                        Image {
                            visible: root.backend.subtitleTranslationActive
                            source: root.iconRoot + "check.svg"
                            sourceSize: Qt.size(20, 20)
                            Layout.preferredWidth: 20
                            Layout.preferredHeight: 20
                        }
                    }
                    background: Rectangle {
                        radius: 7
                        color: translateSubtitles.hovered ? "#343638" : "transparent"
                        border.color: translateSubtitles.visualFocus ? "#009df7" : "transparent"
                        border.width: 2
                    }
                    onClicked: root.backend.useTranslatedSubtitles()
                }
                Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: 12
                    Layout.rightMargin: 12
                    visible: ["working", "bitmap", "unsupported", "unavailable", "modelUnavailable", "failed"].includes(root.backend.subtitleStatus)
                    text: {
                        switch (root.backend.subtitleStatus) {
                        case "working": return root.russian ? qsTr("Переводим…") : qsTr("Translating…")
                        case "bitmap": return root.russian ? qsTr("Нужна текстовая дорожка субтитров") : qsTr("Choose a text subtitle track")
                        case "unsupported": return root.russian ? qsTr("Эту дорожку пока нельзя перевести") : qsTr("This track cannot be translated yet")
                        case "unavailable": return root.russian ? qsTr("Codex недоступен") : qsTr("Codex is unavailable")
                        case "modelUnavailable": return root.russian ? qsTr("Выбранная модель недоступна") : qsTr("Selected model is unavailable")
                        default: return root.russian ? qsTr("Не удалось перевести субтитры") : qsTr("Could not translate subtitles")
                        }
                    }
                    color: root.backend.subtitleStatus === "working" ? "#b9bec4" : "#ff9a9a"
                    wrapMode: Text.WordWrap
                    font.pixelSize: 13
                }

                Repeater {
                    model: [
                        {
                            title: root.russian ? qsTr("Выкл.") : qsTr("Off"),
                            value: "no"
                        },
                        {
                            title: root.russian ? qsTr("Авто") : qsTr("Auto"),
                            value: "auto"
                        }
                    ]
                    delegate: ItemDelegate {
                        id: subtitleOption
                        required property var modelData
                        Layout.fillWidth: true
                        implicitHeight: 44
                        enabled: root.backend.mediaLoaded
                        text: modelData.title
                        Accessible.name: text
                        background: Rectangle {
                            radius: 7
                            color: subtitleOption.hovered ? "#343638" : "transparent"
                            border.color: subtitleOption.visualFocus ? "#009df7" : "transparent"
                            border.width: 2
                        }
                        contentItem: RowLayout {
                            Label {
                                Layout.fillWidth: true
                                text: subtitleOption.text
                                color: "#ffffff"
                            }
                            Image {
                                visible: root.backend.subtitleSelection === subtitleOption.modelData.value
                                source: root.iconRoot + "check.svg"
                                sourceSize: Qt.size(20, 20)
                                Layout.preferredWidth: 20
                                Layout.preferredHeight: 20
                            }
                        }
                        onClicked: {
                            if (modelData.value === "no")
                                root.backend.disableSubtitles();
                            else
                                root.backend.selectAutomaticTrack("sub");
                        }
                    }
                }
                Repeater {
                    model: root.backend.subtitleTracks
                    delegate: ItemDelegate {
                        id: subtitleTrack
                        required property int trackId
                        required property string label
                        required property bool selected
                        Layout.fillWidth: true
                        implicitHeight: 44
                        text: (root.russian ? qsTr("Дорожка %1") : qsTr("Track %1")).arg(trackId) + (label.length ? " — " + label : "")
                        Accessible.name: text
                        background: Rectangle {
                            radius: 7
                            color: subtitleTrack.hovered ? "#343638" : "transparent"
                            border.color: subtitleTrack.visualFocus ? "#009df7" : "transparent"
                            border.width: 2
                        }
                        contentItem: RowLayout {
                            Label {
                                Layout.fillWidth: true
                                text: subtitleTrack.text
                                color: "#ffffff"
                                elide: Text.ElideRight
                            }
                            Image {
                                visible: subtitleTrack.selected && root.backend.subtitleSelection !== "auto"
                                source: root.iconRoot + "check.svg"
                                sourceSize: Qt.size(20, 20)
                                Layout.preferredWidth: 20
                                Layout.preferredHeight: 20
                            }
                        }
                        onClicked: root.backend.selectTrack("sub", trackId)
                    }
                }
                ItemDelegate {
                    id: addSubtitle
                    Layout.fillWidth: true
                    implicitHeight: 44
                    enabled: root.backend.mediaLoaded
                    text: root.russian ? qsTr("Добавить файл субтитров") : qsTr("Add subtitle file")
                    Accessible.name: text
                    contentItem: Label {
                        text: addSubtitle.text
                        color: "#62beff"
                    }
                    background: Rectangle {
                        radius: 7
                        color: addSubtitle.hovered ? "#343638" : "transparent"
                        border.color: addSubtitle.visualFocus ? "#009df7" : "transparent"
                        border.width: 2
                    }
                    onClicked: subtitleDialog.open()
                }
                ItemDelegate {
                    id: resetSubtitlePosition
                    Layout.fillWidth: true
                    implicitHeight: 44
                    visible: root.backend.subtitleMovable && Math.abs(root.backend.subtitlePosition - 85) >= 0.1
                    text: root.russian ? qsTr("Вернуть положение субтитров") : qsTr("Reset subtitle position")
                    Accessible.name: text
                    contentItem: Label {
                        text: resetSubtitlePosition.text
                        color: "#62beff"
                    }
                    background: Rectangle {
                        radius: 7
                        color: resetSubtitlePosition.hovered ? "#343638" : "transparent"
                        border.color: resetSubtitlePosition.visualFocus ? "#009df7" : "transparent"
                        border.width: 2
                    }
                    onClicked: root.backend.resetSubtitlePosition()
                }
            }

            ColumnLayout {
                visible: root.page === 3
                Layout.fillWidth: true
                spacing: 14

                Label {
                    Layout.alignment: Qt.AlignHCenter
                    text: Math.round(root.backend.speed * 100) / 100 + "×"
                    color: "#ffffff"
                    font.pixelSize: 28
                    font.weight: Font.DemiBold
                }
                Slider {
                    id: speedSlider
                    objectName: "speedSlider"
                    Layout.fillWidth: true
                    implicitHeight: 44
                    from: 0.25
                    to: 4
                    stepSize: 0.05
                    value: root.backend.speed
                    enabled: root.backend.engineReady
                    Accessible.name: root.russian ? qsTr("Скорость воспроизведения") : qsTr("Playback speed")
                    onMoved: root.backend.changeSpeed(Math.round(value * 20) / 20)
                    background: Item {
                        x: speedSlider.leftPadding
                        y: speedSlider.topPadding + speedSlider.availableHeight / 2 - 3
                        width: speedSlider.availableWidth
                        height: 6
                        Rectangle {
                            anchors.fill: parent
                            radius: 3
                            color: "#74777a"
                        }
                        Rectangle {
                            width: speedSlider.visualPosition * parent.width
                            height: parent.height
                            radius: 3
                            color: "#009df7"
                        }
                    }
                    handle: Rectangle {
                        x: speedSlider.leftPadding + speedSlider.visualPosition * (speedSlider.availableWidth - width)
                        y: speedSlider.topPadding + speedSlider.availableHeight / 2 - height / 2
                        width: 14
                        height: 14
                        radius: 7
                        color: "#ffffff"
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    Label {
                        text: "0.25×"
                        color: "#b9bec4"
                    }
                    Item {
                        Layout.fillWidth: true
                    }
                    Label {
                        text: "4×"
                        color: "#b9bec4"
                    }
                }
                GridLayout {
                    Layout.fillWidth: true
                    columns: 4
                    rowSpacing: 6
                    columnSpacing: 6
                    Repeater {
                        model: [0.5, 0.75, 1, 1.25, 1.5, 2, 3]
                        delegate: Button {
                            id: preset
                            required property real modelData
                            objectName: modelData === 2 ? "speedPreset2" : ""
                            Layout.fillWidth: true
                            implicitHeight: 44
                            text: modelData + "×"
                            font.pixelSize: 14
                            enabled: root.backend.engineReady
                            checked: Math.abs(root.backend.speed - modelData) < 0.001
                            contentItem: Label {
                                text: preset.text
                                color: "#ffffff"
                                font: preset.font
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                radius: 7
                                color: preset.checked ? "#009df7" : "#343638"
                                border.color: preset.visualFocus ? "#009df7" : "transparent"
                                border.width: 2
                            }
                            onClicked: root.backend.changeSpeed(modelData)
                        }
                    }
                }
            }

            ColumnLayout {
                visible: root.page === 4
                Layout.fillWidth: true
                spacing: 2

                Label {
                    Layout.topMargin: 10
                    Layout.bottomMargin: 4
                    text: root.russian ? qsTr("Генерация кадров") : qsTr("Frame generation")
                    color: "#b9bec4"
                    font.pixelSize: 13
                }
                Repeater {
                    model: [
                        { id: "auto", label: root.russian ? qsTr("Авто") : qsTr("Auto") },
                        { id: "on", label: root.russian ? qsTr("Вкл.") : qsTr("On") },
                        { id: "off", label: root.russian ? qsTr("Выкл.") : qsTr("Off") }
                    ]
                    delegate: ItemDelegate {
                        id: framegenOption
                        required property var modelData
                        Layout.fillWidth: true
                        implicitHeight: 44
                        text: modelData.label
                        Accessible.name: text
                        onClicked: root.backend.changeFramegen(modelData.id)
                        contentItem: RowLayout {
                            Label {
                                Layout.fillWidth: true
                                text: framegenOption.text
                                color: "#ffffff"
                            }
                            Image {
                                visible: root.backend.framegenChoice === framegenOption.modelData.id
                                source: root.iconRoot + "check.svg"
                                sourceSize: Qt.size(20, 20)
                                Layout.preferredWidth: 20
                                Layout.preferredHeight: 20
                            }
                        }
                    }
                }

                Label {
                    visible: root.backend.framegenStatus === "loading" || root.backend.framegenStatus === "prepared" || root.backend.framegenStatus === "guarded"
                    Layout.fillWidth: true
                    Layout.topMargin: 4
                    wrapMode: Text.WordWrap
                    text: root.backend.framegenStatus === "loading" ? (root.russian ? qsTr("Подготовка в фоне") : qsTr("Preparing in the background")) : root.backend.framegenStatus === "prepared" ? (root.russian ? qsTr("Готово для следующего видео") : qsTr("Ready for the next video")) : (root.russian ? qsTr("Недоступно для этого видео") : qsTr("Unavailable for this video"))
                    color: "#b9bec4"
                    font.pixelSize: 13
                }

                Label {
                    Layout.topMargin: 10
                    Layout.bottomMargin: 4
                    text: root.russian ? qsTr("Масштабирование") : qsTr("Upscaling")
                    color: "#b9bec4"
                    font.pixelSize: 13
                }
                Repeater {
                    model: [
                        { id: "auto", label: root.russian ? qsTr("Авто") : qsTr("Auto") },
                        { id: "off", label: root.russian ? qsTr("Выкл.") : qsTr("Off") },
                        { id: "fsr", label: "FSR" },
                        { id: "anime4k_a", label: "Anime4K A" },
                        { id: "anime4k_b", label: "Anime4K B" },
                        { id: "anime4k_c", label: "Anime4K C" },
                        { id: "anime4k_aa", label: "Anime4K A+A" },
                        { id: "anime4k_bb", label: "Anime4K B+B" },
                        { id: "anime4k_ca", label: "Anime4K C+A" },
                        { id: "anime4k_gan_2x", label: "Anime4K GAN 2×" },
                        { id: "anime4k_gan_3x", label: "Anime4K GAN 3×" },
                        { id: "anime4k_gan_4x", label: "Anime4K GAN 4×" }
                    ]
                    delegate: ItemDelegate {
                        id: shaderOption
                        required property var modelData
                        Layout.fillWidth: true
                        implicitHeight: 44
                        text: modelData.label
                        Accessible.name: text
                        onClicked: root.backend.changeShader(modelData.id)
                        contentItem: RowLayout {
                            Label {
                                Layout.fillWidth: true
                                text: shaderOption.text
                                color: "#ffffff"
                            }
                            Image {
                                visible: root.backend.shaderChoice === shaderOption.modelData.id
                                source: root.iconRoot + "check.svg"
                                sourceSize: Qt.size(20, 20)
                                Layout.preferredWidth: 20
                                Layout.preferredHeight: 20
                            }
                        }
                    }
                }

                Label {
                    Layout.topMargin: 6
                    Layout.bottomMargin: 4
                    text: root.russian ? qsTr("Тип видео") : qsTr("Video type")
                    color: "#b9bec4"
                    font.pixelSize: 13
                }
                Repeater {
                    model: [
                        { id: "auto", label: root.russian ? qsTr("Авто") : qsTr("Auto") },
                        { id: "anime", label: root.russian ? qsTr("Аниме") : qsTr("Anime") },
                        { id: "movie", label: root.russian ? qsTr("Фильм") : qsTr("Movie") }
                    ]
                    delegate: ItemDelegate {
                        id: contentOption
                        required property var modelData
                        Layout.fillWidth: true
                        implicitHeight: 44
                        text: modelData.label
                        Accessible.name: text
                        onClicked: root.backend.changeContent(modelData.id)
                        contentItem: RowLayout {
                            Label {
                                Layout.fillWidth: true
                                text: contentOption.text
                                color: "#ffffff"
                            }
                            Image {
                                visible: root.backend.contentChoice === contentOption.modelData.id
                                source: root.iconRoot + "check.svg"
                                sourceSize: Qt.size(20, 20)
                                Layout.preferredWidth: 20
                                Layout.preferredHeight: 20
                            }
                        }
                    }
                }

                Label {
                    visible: root.backend.gpuError.length > 0
                    Layout.fillWidth: true
                    Layout.topMargin: 8
                    wrapMode: Text.WordWrap
                    text: root.russian ? qsTr("Не удалось применить FX") : qsTr("Could not apply FX")
                    color: "#ff9a9a"
                }
            }
        }
    }

    FileDialog {
        id: subtitleDialog
        title: root.russian ? qsTr("Добавить субтитры") : qsTr("Add subtitles")
        onAccepted: root.backend.addSubtitle(selectedFile.toString())
    }
}
