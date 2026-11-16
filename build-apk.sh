#!/bin/sh
# Build APK Usap (debug). Hasilnya usap-<versi>-debug.apk di folder ini.
#
# Tanpa setelan apa pun: build di komputer ini dengan Gradle wrapper.
#   Butuh JDK 17 dan Android SDK 34 (ANDROID_HOME, atau Android Studio sudah terpasang).
#
# Dengan USAP_BUILD_HOST=<host ssh>: build di mesin lain lewat SSH, di dalam container Docker.
#   USAP_BUILDER_IMAGE  image berisi JDK 17 + Android SDK 34 di /opt/android-sdk (bawaan: usap-android-builder)
#   USAP_GRADLE_VOLUME  volume Docker untuk cache Gradle (bawaan: usap-android-gradle)
#   Kunci debug disimpan di ~/work/usap-keys di mesin itu, supaya APK baru bisa dipasang menimpa yang lama.
#
# Setelan pribadi boleh ditaruh di build.local.env (tidak ikut git).
set -e
ROOT=$(cd "$(dirname "$0")" && pwd)
[ -f "$ROOT/build.local.env" ] && . "$ROOT/build.local.env"

if [ -z "$USAP_BUILD_HOST" ]; then
    cd "$ROOT/android"
    chmod +x gradlew
    ./gradlew --no-daemon -q assembleDebug
    cp app/build/outputs/apk/debug/*.apk "$ROOT/"
    ls -la "$ROOT"/*.apk
    exit 0
fi

HOST=$USAP_BUILD_HOST
IMAGE=${USAP_BUILDER_IMAGE:-usap-android-builder}
VOLUME=${USAP_GRADLE_VOLUME:-usap-android-gradle}

ssh "$HOST" "mkdir -p ~/work/usap/android"
rsync -az --delete --exclude .gradle --exclude build --exclude local.properties \
    "$ROOT/android/" "$HOST:~/work/usap/android/"

ssh "$HOST" "mkdir -p ~/work/usap-keys && chmod 700 ~/work/usap-keys && cd ~/work && \
  timeout 900 docker run --rm --name usap-build \
    -v \$PWD/usap/android:/src -w /src \
    -v \$PWD/usap-keys:/keys \
    -v $VOLUME:/root/.gradle \
    $IMAGE sh -c '\
      [ -f /keys/debug.keystore ] || keytool -genkeypair -keystore /keys/debug.keystore -storepass android \
        -alias androiddebugkey -keypass android -keyalg RSA -keysize 2048 -validity 10000 \
        -dname \"CN=Usap Debug\" > /dev/null 2>&1; \
      printf \"sdk.dir=/opt/android-sdk\nusap.keystore=/keys/debug.keystore\n\" > local.properties && \
      chmod +x gradlew && ./gradlew --no-daemon -q assembleDebug'"

scp -q "$HOST:~/work/usap/android/app/build/outputs/apk/debug/*.apk" "$ROOT/"
ls -la "$ROOT"/*.apk
