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

PlasmoidItem {
    id: root
    property string outputText
    property bool isActive: false

    // Constants
    readonly property int defaultInterval: 7000
    readonly property int toggleInterval: 1500
    readonly property string checkCommand: 'sh -c "test -f ~/.config/pipewire/filter-chain.conf.d/source-gtcrn-smart.conf"'

    // Function to run the command
    function runCommand() {
        executable.exec(checkCommand)
    }

    // Function to toggle the noise reduction
    function toggle() {
        var command = isActive ? 'stop' : 'start'
        executable.exec('/usr/bin/pipewire-noise-remove ' + command)
        timer.interval = toggleInterval  // Shorten the interval for quick feedback
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

    Connections {
        function onExited(sourceName, exitCode, exitStatus, stdout, stderr) {
            if (sourceName == checkCommand) {
                Qt.callLater(function() {
                    root.outputText = stdout;
                    root.isActive = (exitCode === 0);
                });
            } else {
                // Was a toggle command, force immediate check
                runCommand()
            }
            timer.restart();
        }

        target: executable
    }

    // Timer to periodically run the command
    Timer {
        id: timer
        interval: defaultInterval
        onTriggered: runCommand()
        Component.onCompleted: {
            triggered()
        }
    }

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
