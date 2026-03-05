cmake_minimum_required(VERSION 3.22.1)
project(rust_bridge)

add_library(apollos_core SHARED IMPORTED)
set_target_properties(
    apollos_core PROPERTIES
    IMPORTED_LOCATION "${CMAKE_SOURCE_DIR}/../jniLibs/${ANDROID_ABI}/libapollos_core.so"
)

add_library(rust_bridge SHARED rust_bridge.cpp)
find_library(log-lib log)

target_link_libraries(rust_bridge PRIVATE apollos_core ${log-lib})
