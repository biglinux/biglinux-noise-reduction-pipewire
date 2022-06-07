/*
 * SPDX-FileCopyleftText: 2022 Bruno Gonçalves <bigbruno@gmail.com> and Rafael Ruscher <rruscher@gmail.com>
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

import QtQuick 2.1
import QtQuick.Layouts 1.0
import org.kde.plasma.plasmoid 2.0
import org.kde.plasma.core 2.0 as PlasmaCore
import org.kde.plasma.components 2.0 as PlasmaComponent

Item {
    id: root

	PlasmaCore.DataSource {
		id: executable
		engine: "executable"
		connectedSources: []
		onNewData: {
			var exitCode = data["exit code"]
			var exitStatus = data["exit status"]
			var stdout = data["stdout"]
			var stderr = data["stderr"]
			exited(sourceName, exitCode, exitStatus, stdout, stderr)
			disconnectSource(sourceName) // cmd finished
		}
		function exec(cmd) {
			if (cmd) {
				connectSource(cmd)
			}
		}
		signal exited(string cmd, int exitCode, int exitStatus, string stdout, string stderr)
	}

	property string outputText: ''
	Connections {
		target: executable
		onExited: {
			outputText = stdout
			timer.restart()
		}
	}

	function runCommand() {
		
		// Change to run your command
		executable.exec('ps -x | grep "/bin/bash /usr/bin/[p]ipewire-noise-remove"')
	}

	Timer {
		id: timer
		
		// Wait in ms
		interval: 7000
		onTriggered: runCommand()
		Component.onCompleted: {
			triggered()
		}
	}

    Plasmoid.icon: outputText ? 'big-noise-reduction-on' : 'big-noise-reduction-off'
    Plasmoid.preferredRepresentation: Plasmoid.compactRepresentation

    // Active = in systray and Passive in notification area
    Plasmoid.status: {
        //return PlasmaCore.Types.ActiveStatus;
        return PlasmaCore.Types.PassiveStatus;
     }

    function toggle() {
        if (outputText) {
            
            executable.exec('systemctl --user stop noise-reduction-pipewire')
            timer.interval = 500
        } else {
            executable.exec('systemctl --user start noise-reduction-pipewire')
            timer.interval = 500
        }
    }


    Plasmoid.compactRepresentation: PlasmaCore.IconItem {
        active: compactMouseArea.containsMouse
        source: plasmoid.icon
        
        MouseArea {
            id: compactMouseArea
            anchors.fill: parent
            hoverEnabled: true
            acceptedButtons: Qt.LeftButton | Qt.MiddleButton
            onClicked: toggle()
        }
    }


}
