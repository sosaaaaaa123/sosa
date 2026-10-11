# Versione Audio Unit (AUv2) di Sosa Tune, per Logic Pro e GarageBand.
#
# Logic carica solo plugin Audio Unit (.component). Il plugin viene
# "avvolto" con clap-wrapper (https://github.com/free-audio/clap-wrapper,
# licenza MIT): il .component contiene dentro di se' SosaTune.clap (lo stesso
# plugin delle versioni VST3 e CLAP) e lo presenta a Logic come Audio Unit.
#
# Uso (lo fa la compilazione automatica su GitHub):
#   cmake -S installer/auv2 -B build-auv2 -DCMAKE_BUILD_TYPE=Release \
#         -DCLAP_WRAPPER_DIR=/percorso/clap-wrapper \
#         -DSOSATUNE_CLAP=/percorso/SosaTune.clap
#   cmake --build build-auv2

cmake_minimum_required(VERSION 3.21)
set(CMAKE_OSX_DEPLOYMENT_TARGET 10.13 CACHE STRING "Versione minima di macOS")
set(CMAKE_OSX_ARCHITECTURES "arm64;x86_64" CACHE STRING "Apple Silicon + Intel")
project(sosatune-auv2 VERSION 1.0.0 LANGUAGES C CXX OBJC OBJCXX)

# L'SDK Audio Unit di Apple richiede il C++17.
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_OBJCXX_STANDARD 17)
set(CMAKE_OBJCXX_STANDARD_REQUIRED ON)
set(CLAP_WRAPPER_CXX_STANDARD 17 CACHE STRING "")

if (NOT DEFINED CLAP_WRAPPER_DIR OR NOT DEFINED SOSATUNE_CLAP)
    message(FATAL_ERROR "Servono -DCLAP_WRAPPER_DIR=... e -DSOSATUNE_CLAP=...")
endif ()

set(CLAP_WRAPPER_DOWNLOAD_DEPENDENCIES TRUE CACHE BOOL "Scarica gli SDK necessari")
add_subdirectory(${CLAP_WRAPPER_DIR} clap-wrapper)

add_library(SosaTune_auv2 MODULE)
target_add_auv2_wrapper(
        TARGET SosaTune_auv2
        OUTPUT_NAME "SosaTune"
        BUNDLE_IDENTIFIER "com.vladislav.sosatune.auv2"
        BUNDLE_VERSION "1.0.0"
        MANUFACTURER_NAME "Vladislav"
        # Codici a 4 lettere che identificano il plugin in Logic (devono restare
        # sempre gli stessi, altrimenti i progetti non lo ritrovano).
        MANUFACTURER_CODE "Vlad"
        SUBTYPE_CODE "SsTn"
        # aufx = effetto audio
        INSTRUMENT_TYPE "aufx"
        MACOS_EMBEDDED_CLAP_LOCATION "${SOSATUNE_CLAP}"
)
