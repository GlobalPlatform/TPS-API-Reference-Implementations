# Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
# SPDX-License-Identifier: MIT

add_executable(c_example c_example.c ../tpsc_client_api.h)

include_directories(${CMAKE_SOURCE_DIR}/tps_client_c)
target_link_libraries(c_example tps_client_api_c rot13_connector)
