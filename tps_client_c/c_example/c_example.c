/****************************************************************************************************
* Copyright (c) 2022, Qualcomm Innovation Center, Inc. All rights reserved.
*
* Permission is hereby granted, free of charge, to any person obtaining a copy of this software
* and associated documentation files (the “Software”), to deal in the Software without
* restriction, including without limitation the rights to use, copy, modify, merge, publish,
* distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
* Software is furnished to do so, subject to the following conditions:
*
* The above copyright notice and this permission notice (including the next
* paragraph) shall be included in all copies or substantial portions of the
* Software.
*
* THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
* BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
* NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
* DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
* OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
***************************************************************************************************/
/*
 * A simple example of use of TPS Client API from C code
 */
#include <stdio.h>
#include <stdint.h>
#include <strings.h>
#include <libc.h>
#include "tpsc_client_api.h"

/* Defines a ROT13 service called "GPP ROT13" using the normative namespace 87bae713-b08f-5e28-b9ee-4aa6e202440e */
#define SERVICE_ID_GPP_ROT13 { .bytes = { 0x87, 0xba, 0xe7, 0x13, 0xb0, 0x8f, 0x5e, 0x28, \
                                          0xb9, 0xee, 0x4a, 0xa6, 0xe2, 0x02, 0x44, 0x0e } }

#define TRANSACTION_BUFFER_SIZE (256)
#define ARRAY_SIZE(val, type)   (sizeof(val)/sizeof(type))

/* A real program would use a CBOR encoder and decoder. For simplicity, I have worked out the CBOR for input to the
 * Service and the expected output.
 *
 * The input (in CBOR Diagnostic format) is: 10({1:"Thisgoestoeleven"}).
 * Expected output (in CBOR diagnostic format): 10({1:"Guvftbrfgbryrira"})
 */
#define INPUT_MSG  {0xCA, /* tag(10) */\
                    0xA1, /* map(1) */\
                    0x01, /* unsigned 1 */\
                    0x70, /* tstr(16) */\
                    0x54, 0x68, 0x69, 0x73, 0x67, 0x6F, 0x65, 0x73, 0x74, \
                    0x6F, 0x65, 0x6C, 0x65, 0x76, 0x65, 0x6E /* "Thisgoestoeleven" */ \
                    }
#define EXPECT_MSG {0xCA, /* tag(10) */\
                    0xA1, /* map(1) */\
                    0x01, /* unsigned 1 */\
                    0x70, /* tstr(16) */\
                    0x47, 0x75, 0x76, 0x66, 0x74, 0x62, 0x72, 0x66, 0x67, \
                    0x62, 0x72, 0x79, 0x72, 0x69, 0x72, 0x61 /* "Guvftbrfgbryrira" */ \
                    }

// Perform Service Discovery
uint32_t DoServiceDiscovery(TPSC_ServiceIdentifier* service_id) {

    // Set up the service selector
    TPSC_ServiceSelector selector = {
            .service_id = SERVICE_ID_GPP_ROT13,
            .secure_component_instance = TPSC_UUID_NIL,
            .secure_component_type = TPSC_UUID_NIL,
            .service_version_range = {
                    .lowest_acceptable_version = { .tag = Inclusive,
                                                   .inclusive = {
                                                        .major_version = 0,
                                                        .minor_version = 0,
                                                        .patch_version = 1
                    }},
                    .first_excluded_version = { .tag = Inclusive,
                                                .inclusive = {
                                                    .major_version = 1,
                                                    .minor_version = 1,
                                                    .patch_version = 1
                    }},
                    .last_excluded_version = { .tag = Exclusive,
                                               .exclusive = {
                                                    .major_version = 1,
                                                    .minor_version = 2,
                                                    .patch_version = 0
                    }},
                    .highest_acceptable_version = { .tag = Exclusive,
                                                    .exclusive = {
                                                        .major_version = 2,
                                                        .minor_version = 0,
                                                        .patch_version = 0
                    }}
            }
    };
    static TPSC_ServiceIdentifier services_available[3];
    size_t no_of_services = sizeof(services_available) / sizeof(TPSC_ServiceIdentifier);

    // Call Service Discovery
    uint32_t retval = TPSC_ServiceDiscovery(&selector, &no_of_services, services_available);

    // The service we want is the first one in the list
    *service_id = services_available[0];
    return retval;
}

/* Print the contents of a message */
void PrintMessage(char *heading, uint8_t* msg, size_t len) {
    printf("%s\n", heading);
    for (int i = 0; i < len; i++) {
        printf("%x, ", msg[i]);
        /* Put a carriage return every 8 bytes */
        if (i % 8 == 0) {
            printf("\n");
        }
    }
    printf("\n");
}

int PrepareMessage(const uint8_t *src, const size_t src_len, uint8_t *dest, const size_t dest_len) {
    if (src_len <= dest_len) {
        memcpy(dest, src, src_len);
        return TRUE;
    } else
        return FALSE;
}

int main(int argc, char** argv) {
    TPSC_ServiceIdentifier svc_id;
    uint8_t send_msg[] = INPUT_MSG;
    PrintMessage("Input Message", send_msg, 20 /* ARRAY_SIZE(send_msg, uint8_t)*/);

    if (DoServiceDiscovery(&svc_id) == TPSC_SUCCESS) {
        TPSC_Session session;
        if (TPSC_OpenSession(&(svc_id.service_instance), TPSC_LOGIN_PUBLIC, NULL, &session) == TPSC_SUCCESS) {
            void *send_buffer = malloc(TRANSACTION_BUFFER_SIZE);
            void *recv_buffer = malloc(TRANSACTION_BUFFER_SIZE);
            TPSC_MessageBuffer send_buf;
            TPSC_MessageBuffer recv_buf;
            if ((TPSC_InitializeTransaction(&send_buf, send_buffer, TRANSACTION_BUFFER_SIZE) == TPSC_SUCCESS) &&
                (TPSC_InitializeTransaction(&recv_buf, recv_buffer, TRANSACTION_BUFFER_SIZE) == TPSC_SUCCESS)){
                PrepareMessage(send_msg, 20 /*ARRAY_SIZE(send_msg, uint8_t)*/, send_buffer, TRANSACTION_BUFFER_SIZE);
                send_buf.size = 20; //sizeof(ARRAY_SIZE(send_msg, uint8_t));
                if (TPSC_ExecuteTransaction(&session, &send_buf, &recv_buf) == TPSC_SUCCESS) {
                    PrintMessage("Received Message", recv_buf.message, recv_buf.size);
                } else {
                    printf("Transaction failed!");
                }
            }
        }
    } else {
        printf("Service discovery failed");
    }
}
