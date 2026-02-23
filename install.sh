#!/bin/bash
# THAT'S FKING IT I AM REWRITING IN BASH BECAUSE WTH

trap "tput cnorm; exit" SIGINT SIGTERM

init_dialog() {

    dialogrc_content='use_colors = ON
screen_color = (WHITE,BLACK, ON)
border_color = (BLACK,WHITE, ON)
'

    dialogrc_path="/tmp/shw_dialog_theme"

    # Write the content string to the file
    printf "%s\n" "$dialogrc_content" > "$dialogrc_path"

    # Export DIALOGRC so dialog picks it up
    export DIALOGRC="$dialogrc_path"
}

gather_hw_info() {
    cpu_arch=$(lscpu -J | jq -r '.lscpu[] | select(.field=="Architecture:") | .data')

    if [[ $cpu_arch == *86* ]]; then
        cpu_manufacturer=$(lscpu -J | jq -r '.lscpu[] | select(.field=="Vendor ID:") | .data')
        echo "CISC CPU detected: $cpu_arch, Vendor: $cpu_manufacturer"
    elif [[ $cpu_arch == arm* ]] || [[ $cpu_arch == aarch64 ]]; then
        echo "ARM CPU detected: $cpu_arch — no microcode packages needed"
    else
        echo "Unsupported or niche CPU: $cpu_arch"
    fi
}

install_package() {
    pacman --noconfirm --needed -Sy $1
}

init() {
    echo -e "\e[1;32m=== Shipwright Installer ===\e[0m"
    echo "Initializing pacman"
    #pacman-key --init
    #pacman-key --populate archlinux
    #install_package archlinux-keyring
    #install_package dialog jq
    init_dialog   
    gather_hw_info
}

message() {
    local text="$1"
    local title="${2:-Message}"
    tput civis
    dialog --no-shadow --colors --title "$title" --msgbox "$text" 10 50
    tput cnorm
}

question() {
    local text="$1"
    local title="${2:-Message}"
    tput civis
    dialog --no-shadow --colors --title "$title" --yesno "$text" 10 50
    local result=$?
    tput cnorm
    clear
    return $result
}

#if [[ $UID != 0 ]]; then
#    echo -e "\e[1;91mERROR: \e[0mShipwright is unable to run without root priviliges. Please rerun as root."
#    exit 1
#fi

init
question "Is bash good?"
if [[ $? == 1 ]]; then
    echo "why..."
else
    echo "yay"
fi

