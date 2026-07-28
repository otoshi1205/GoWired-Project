// Emits the exact bytes the MySensors C++ library puts on the wire for the
// messages this firmware sends, as Rust test fixtures.
#include "MyMessage.h"
#include <stdio.h>
#include <string.h>

static void dump(const char* name, MyMessage& m)
{
    const uint8_t len = m.getLength();
    const uint8_t* raw = (const uint8_t*)&m;
    printf("    // %s\n    (\"%s\", &[", name, name);
    for (uint8_t i = 0; i < HEADER_SIZE + len; i++) {
        printf("0x%02X%s", raw[i], i + 1 < HEADER_SIZE + len ? ", " : "");
    }
    printf("]),\n");
}

static MyMessage& build(MyMessage& m, uint8_t sender, uint8_t dest, uint8_t sensor,
                        mysensors_command_t cmd, uint8_t type)
{
    m.setSender(sender);
    m.setDestination(dest);
    m.setSensor(sensor);
    (void)m.setVersion();
    m.setCommand(cmd);
    m.setType(type);
    m.setLast(sender);
    m.setEcho(false);
    m.setRequestEcho(false);
    m.setSigned(false);
    return m;
}

int main()
{
    MyMessage m;
    memset(&m, 0, sizeof m);

    dump("relay0_status_true", build(m, 12, 0, 0, C_SET, V_STATUS).set(true));
    memset(&m, 0, sizeof m);
    dump("relay1_status_false", build(m, 12, 0, 1, C_SET, V_STATUS).set(false));
    memset(&m, 0, sizeof m);
    dump("percentage_63", build(m, 12, 0, 0, C_SET, V_PERCENTAGE).set((uint32_t)63));
    memset(&m, 0, sizeof m);
    dump("watt_460", build(m, 12, 0, 10, C_SET, V_WATT).set(460.0f, 0));
    memset(&m, 0, sizeof m);
    dump("temp_21_5", build(m, 12, 0, 12, C_SET, V_TEMP).set(21.5f, 1));
    memset(&m, 0, sizeof m);
    dump("present_relay1", build(m, 12, 0, 0, C_PRESENTATION, S_BINARY).set("Relay 1"));
    memset(&m, 0, sizeof m);
    dump("present_cover", build(m, 12, 0, 0, C_PRESENTATION, S_COVER).set("Roller Shutter"));
    memset(&m, 0, sizeof m);
    dump("sketch_name", build(m, 12, 0, NODE_SENSOR_ID, C_INTERNAL, I_SKETCH_NAME).set("GoWired Module"));
    memset(&m, 0, sizeof m);
    dump("sketch_version", build(m, 12, 0, NODE_SENSOR_ID, C_INTERNAL, I_SKETCH_VERSION).set("3.0"));
    memset(&m, 0, sizeof m);
    dump("request_status", build(m, 12, 0, 0, C_REQ, V_STATUS).set(""));
    memset(&m, 0, sizeof m);
    dump("rgb_ffffff", build(m, 12, 0, 0, C_SET, V_RGB).set("ffffff"));
    memset(&m, 0, sizeof m);
    dump("id_request", build(m, 255, 0, NODE_SENSOR_ID, C_INTERNAL, I_ID_REQUEST).set(""));
    memset(&m, 0, sizeof m);
    dump("node_presentation", build(m, 12, 0, NODE_SENSOR_ID, C_PRESENTATION, S_ARDUINO_NODE).set("2.4.0"));
    memset(&m, 0, sizeof m);
    dump("config_text", build(m, 12, 0, 20, C_SET, V_TEXT).set("CONFIG INIT"));
    memset(&m, 0, sizeof m);
    dump("heartbeat_response", build(m, 12, 0, NODE_SENSOR_ID, C_INTERNAL, I_HEARTBEAT_RESPONSE).set((uint32_t)123456));
    memset(&m, 0, sizeof m);
    dump("pong", build(m, 12, 0, NODE_SENSOR_ID, C_INTERNAL, I_PONG).set((uint8_t)1));
    printf("    // sizeof(MyMessage) = %zu, HEADER_SIZE = %u, MAX_PAYLOAD = %u\n",
           sizeof(MyMessage), (unsigned)HEADER_SIZE, (unsigned)MAX_PAYLOAD_SIZE);
    return 0;
}
// Referenced by getCustomString(), which none of the fixtures use.
char convertI2H(const uint8_t i) { return i < 10 ? '0' + i : 'A' + i - 10; }
