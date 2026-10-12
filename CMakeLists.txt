cmake_minimum_required(VERSION 3.28)

project(qplay_native
    VERSION 1.1.0
    LANGUAGES CXX
)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Qt6 6.12 REQUIRED COMPONENTS Quick QuickControls2 QuickDialogs2 OpenGL)
qt_standard_project_setup(REQUIRES 6.12)

include(FetchContent)
file(STRINGS rust-toolchain.toml rustChannelLine REGEX "^channel = ")
string(REGEX REPLACE ".*\"([0-9]+\\.[0-9]+\\.[0-9]+)\".*" "\\1" Rust_TOOLCHAIN "${rustChannelLine}")
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS rust-toolchain.toml)
FetchContent_Declare(Corrosion
    GIT_REPOSITORY https://github.com/corrosion-rs/corrosion.git
    GIT_TAG a1a1aaa057a5da656c06c3d8505b767a4e941709
)
FetchContent_Declare(CxxQt
    GIT_REPOSITORY https://github.com/KDAB/cxx-qt-cmake.git
    GIT_TAG 64e63f560737b3b7f85ef8267c5918e4b8176d5a
)
FetchContent_MakeAvailable(CxxQt)

cxx_qt_import_crate(
    MANIFEST_PATH "${CMAKE_CURRENT_SOURCE_DIR}/src/native/Cargo.toml"
    CRATES qplay_native
    LOCKED
    QT_MODULES Qt::Core Qt::Gui Qt::Qml
)
cxx_qt_import_qml_module(qplay_player_plugin
    URI Qplay.Player
    SOURCE_CRATE qplay_native
)

qt_add_library(qplay_views STATIC)
qt_add_qml_module(qplay_views
    URI Qplay.Views
    OUTPUT_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/Qplay/Views"
    IMPORT_PATH "${CMAKE_CURRENT_BINARY_DIR}/cxxqt/qml_modules"
    SOURCES
        src/native/cpp/videosurface.cpp
        src/native/cpp/videosurface.h
    QML_FILES
        src/native/qml/Main.qml
        src/native/qml/PlaybackMenu.qml
        src/native/qml/PlayerIconButton.qml
        src/native/qml/PlayerScene.qml
        src/native/qml/SettingsPanel.qml
    RESOURCES
        assets/icons/rewind-backward-10.svg
        assets/icons/rewind-forward-10.svg
        assets/icons/check.svg
        assets/icons/chevron-left.svg
        assets/icons/chevron-right.svg
        assets/icons/folder-open.svg
        assets/icons/link.svg
        assets/icons/maximize.svg
        assets/icons/minus-dark.svg
        assets/icons/minus.svg
        assets/icons/player-pause.svg
        assets/icons/player-play.svg
        assets/icons/plus-dark.svg
        assets/icons/plus.svg
        assets/icons/settings.svg
        assets/icons/volume-off.svg
        assets/icons/volume.svg
        assets/icons/x-dark.svg
        assets/icons/x.svg
    DEPENDENCIES Qplay.Player
)
add_dependencies(qplay_views cargo-build_qplay_native)
add_dependencies(qplay_views_qmllint cargo-build_qplay_native)
target_include_directories(qplay_views PRIVATE src/native/cpp)
target_link_libraries(qplay_views
    PRIVATE qplay_native qplay_player_plugin
    PUBLIC Qt::Quick Qt::OpenGL Qt::QuickControls2
)

qt_add_executable(qplay-native WIN32 src/native/cpp/main.cpp)
qt_add_resources(qplay-native "app_icon"
    FILES assets/qplay.ico
)
target_link_libraries(qplay-native
    PRIVATE qplay_views qplay_viewsplugin qplay_native qplay_player_plugin Qt::QuickDialogs2 bcrypt
)

qt_add_executable(qplay-native-acceptance WIN32 tests/native/player.cpp)
target_link_libraries(qplay-native-acceptance
    PRIVATE qplay_views qplay_viewsplugin qplay_native qplay_player_plugin bcrypt
)
