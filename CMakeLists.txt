#***************************************************************************************************
# Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
# SPDX-License-Identifier: MIT
#**************************************************************************************************/

cmake_minimum_required(VERSION 3.20)
project(tps_client_api C)

set(CMAKE_C_STANDARD 99)
set(CMAKE_MODULE_PATH "${CMAKE_SOURCE_DIR}/cmake_rust")
enable_language(Rust)
include(CMakeCargo)

include_directories(tps_client_c)

add_subdirectory(tps_client_c)
add_subdirectory(rot13_connector)
add_subdirectory(tps_client_c/c_example)

