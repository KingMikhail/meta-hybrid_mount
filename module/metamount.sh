MODDIR="${0%/*}"
BASE_DIR="/data/adb/Hybrid-Mount"

mkdir -p "$BASE_DIR"

BINARY="$MODDIR/Hybrid-Mount"
if [ ! -f "$BINARY" ]; then
  echo "Error: Binary Not Found At $BINARY"
  exit 1
fi

chmod 755 "$BINARY"
"$BINARY" 2>&1
EXIT_CODE=$?

if [ "$EXIT_CODE" = "0" ]; then
  /data/adb/ksud kernel notify-module-mounted
fi
exit $EXIT_CODE
