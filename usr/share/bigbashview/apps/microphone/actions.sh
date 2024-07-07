#!/bin/bash

if [[ "$1" = "start" ]]; then
    systemctl --user start noise-reduction-pipewire
elif [[ "$1" = "stop" ]]; then
    systemctl --user stop noise-reduction-pipewire
elif [[ "$1" = "status" ]]; then
    pipewire-noise-remove status
fi
