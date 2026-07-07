package com.zseven.zode

import kotlin.test.Test
import kotlin.test.assertEquals

class ZodeClientTest {
    @Test
    fun defaultsToZodeBinary() {
        val client = ZodeClient()
        assertEquals("zode", client.binary)
    }

    @Test
    fun allowsBinaryOverride() {
        val client = ZodeClient(binary = "/tmp/zode")
        assertEquals("/tmp/zode", client.binary)
    }

    @Test
    fun protocolMethodEnumExposesWireNames() {
        assertEquals("initialize", ProtocolMethod.Initialize.wireName)
        assertEquals("command/exec", ProtocolMethod.CommandExec.wireName)
        assertEquals("mcpServerStatus/list", ProtocolMethod.McpServerStatusList.wireName)
    }
}
