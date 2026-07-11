package com.zseven.zode

import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ProtocolTest {
    @Test
    fun protocolMethodsMatchSchemaInOrder() {
        val schemaRaw = fixturesDir().resolve("protocol.schema.json").readText()
        val schema = testJson.parseToJsonElement(schemaRaw) as JsonObject
        val methods =
            (schema["methods"] as kotlinx.serialization.json.JsonArray).map {
                testJson.decodeFromJsonElement(String.serializer(), it)
            }

        assertEquals(27, ProtocolMethod.entries.size, "SDK must expose 27 methods")
        assertEquals(methods.size, ProtocolMethod.entries.size, "schema/SDK method count differs")
        methods.forEachIndexed { i, want ->
            assertEquals(want, ProtocolMethod.entries[i].wireName, "method $i mismatch")
        }
    }

    @Test
    fun classifierTagsEachKind() {
        assertEquals(
            FrameKind.Response,
            classifyIncomingFrame("""{"jsonrpc":"2.0","id":1,"result":{}}""", testJson).kind,
        )
        assertEquals(
            FrameKind.Error,
            classifyIncomingFrame(
                """{"jsonrpc":"2.0","id":1,"error":{"code":-1,"message":"x"}}""",
                testJson,
            ).kind,
        )
        assertEquals(
            FrameKind.Notification,
            classifyIncomingFrame("""{"jsonrpc":"2.0","method":"turn/started","params":{}}""", testJson).kind,
        )
        assertEquals(
            FrameKind.ServerRequest,
            classifyIncomingFrame(
                """{"jsonrpc":"2.0","id":"a","method":"approval/request","params":{}}""",
                testJson,
            ).kind,
        )
    }

    @Test
    fun classifierRejectsFramesMissingJsonrpc() {
        val bad =
            listOf(
                """{"id":1,"result":{}}""",
                """{"jsonrpc":"1.0","id":1,"result":{}}""",
                """{"jsonrpc":"2.0","id":1}""",
                "not json",
            )
        for (raw in bad) {
            assertFailsWith<InvalidFrameException>("expected rejection for $raw") {
                classifyIncomingFrame(raw, testJson)
            }
        }
    }

    @Test
    fun initializeParamsOmitsNullApprovalPolicy() {
        val omitted =
            testJson.encodeToJsonElement(
                InitializeParams.serializer(),
                InitializeParams(ClientInfo("n", "v"), null),
            ) as JsonObject
        assertFalse(omitted.containsKey("approvalPolicy"), "null approvalPolicy must be omitted")

        val present =
            testJson.encodeToJsonElement(
                InitializeParams.serializer(),
                InitializeParams(ClientInfo("n", "v"), "auto"),
            ) as JsonObject
        assertTrue(present.containsKey("approvalPolicy"), "explicit approvalPolicy must be present")
    }

    @Test
    fun requestFrameAlwaysCarriesJsonrpc() {
        val frame =
            JsonRpcRequest(
                id = kotlinx.serialization.json.JsonPrimitive(1),
                method = "initialize",
                params = buildJsonObject { put("k", "v") },
            )
        val obj = testJson.encodeToJsonElement(JsonRpcRequest.serializer(), frame) as JsonObject
        assertEquals("2.0", (obj["jsonrpc"] as kotlinx.serialization.json.JsonPrimitive).content)
    }
}
