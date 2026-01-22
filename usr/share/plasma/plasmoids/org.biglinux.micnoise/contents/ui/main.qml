/*
 * SPDX-FileCopyleftText: 2022 Bruno Gonçalves <bigbruno@gmail.com> and Rafael Ruscher <rruscher@gmail.com>
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.kirigami as Kirigami
import org.kde.plasma.plasma5support as Plasma5Support
import Qt.labs.folderlistmodel
import Qt.labs.platform

PlasmoidItem {
    id: root
    property string outputText


    // Folder monitoring to check if the filter is active
    FolderListModel {
        id: fileConfigMonitor
        plugin: "qml-folderlistmodel"
        folder: "file://" + StandardPaths.writableLocation(StandardPaths.HomeLocation) + "/.config/pipewire/filter-chain.conf.d"
        nameFilters: ["source-gtcrn-smart.conf", "source-rnnoise-smart.conf"]
    }

    // Function to run the command
    // function runCommand() removed as it is now handled by FolderListModel


    // Function to toggle the noise reduction

    function toggle() {
        var command = isActive ? 'stop' : 'restart'
        executable.exec('systemctl --user ' + command + ' noise-reduction-pipewire')
    }

    preferredRepresentation: fullRepresentation
    // Active = in systray and Passive in notification area
    Plasmoid.status: {
        //return PlasmaCore.Types.ActiveStatus;
        return PlasmaCore.Types.PassiveStatus;
    }

    Plasma5Support.DataSource {
        id: "executable"
        signal exited(string sourceName, int exitCode, int exitStatus, string stdout, string stderr)
        function exec(cmd) {
            connectSource(cmd);
        }

        engine: "executable"
        connectedSources: []
        onNewData: function(sourceName, data) {
            var exitCode = data["exit code"];
            var exitStatus = data["exit status"];
            var stdout = data["stdout"];
            var stderr = data["stderr"];
            exited(sourceName, exitCode, exitStatus, stdout, stderr);
            disconnectSource(sourceName);
        }
    }

    property bool isActive: fileConfigMonitor.count > 0

    fullRepresentation: PlasmoidItem {
        Kirigami.Icon {
            id: icon
            source: isActive ? 'big-noise-reduction-on' : 'big-noise-reduction-off'
            height: Math.min(parent.height, parent.width)
            width: Math.min(parent.height, parent.width)
            anchors.fill: parent
        }

        MouseArea {
            anchors.fill: parent
            onClicked: toggle()
        }
    }
}
