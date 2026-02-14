export KSU_HAS_METAMODULE="true"
export KSU_METAMODULE="Hybrid-Mount"
BASE_DIR="/data/adb/Hybrid-Mount"
BUILTIN_PARTITIONS="system vendor product system_ext odm oem apex"

handle_partition() {
    echo 0 > /dev/null ; true
}

hybrid_handle_partition() {
    partition="$1"

    if [ ! -d "$MODPATH/system/$partition" ]; then
        return
    fi

    if [ -d "$MODPATH/system/$partition" ] && [ ! -L "$MODPATH/system/$partition" ]; then
        ln -sf "$MODPATH/system/$partition" "$MODPATH/$partition"
        ui_print "Handled /$partition"
    fi
}

cleanup_empty_system_dir() {
    if [ -d "$MODPATH/system" ] && [ -z "$(ls -A "$MODPATH/system" 2>/dev/null)" ]; then
        rmdir "$MODPATH/system" 2>/dev/null
        ui_print "Removed Empty /System Directory (Skip System Mount)"
    fi
}

mark_replace() {
  replace_target="$1"
  mkdir -p "$replace_target"
  setfattr -n trusted.overlay.opaque -v y "$replace_target"
}

ui_print "Using Hybrid Mount Meta-Install"

install_module

for partition in $BUILTIN_PARTITIONS; do
    hybrid_handle_partition "$partition"
done

cleanup_empty_system_dir

ui_print "Installation Complete"
