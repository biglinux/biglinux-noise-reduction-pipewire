#!/bin/bash

if [[ "$1" = "start" ]]; then
    systemctl --user start noise-reduction-pipewire
elif [[ "$1" = "stop" ]]; then
    systemctl --user stop noise-reduction-pipewire
else
    pipewire-noise-remove "$1"
fi

# See more options in pipewire-noise-remove --help
