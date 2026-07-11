package com.zseven.zode

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlin.test.fail

/** A fixture request rebuilt through the SDK's own typed serialization. */
private data class RequestCase(
    val id: String,
    val method: ProtocolMethod,
    val params: JsonElement,
)

private fun requestCases(): Map<String, RequestCase> =
    mapOf(
        "initialize.request" to
            RequestCase(
                id = "init",
                method = ProtocolMethod.Initialize,
                params =
                    testJson.encodeToJsonElement(
                        InitializeParams.serializer(),
                        InitializeParams(ClientInfo("fixture", "0.0.0"), "readOnly"),
                    ),
            ),
        "thread-start.request" to
            RequestCase(
                id = "thread",
                method = ProtocolMethod.ThreadStart,
                params = buildJsonObject { put("cwd", "/tmp/project"); put("model", "default") },
            ),
        "fs-read-file.request" to
            RequestCase(
                id = "read",
                method = ProtocolMethod.FsReadFile,
                params = buildJsonObject { put("path", "/tmp/project/hello.txt") },
            ),
        "command-exec.request" to
            RequestCase(
                id = "cmd",
                method = ProtocolMethod.CommandExec,
                params =
                    buildJsonObject {
                        putJsonArray("command") {
                            add("sh")
                            add("-c")
                            add("printf hi")
                        }
                    },
            ),
    )

class FixturesTest {
    @Test
    fun everyRequestFixtureIsCovered() {
        val cases = requestCases()
        val fixtures =
            fixturesDir().listFiles { f -> f.name.endsWith(".request.json") }
                ?: fail("no request fixtures found")
        for (f in fixtures) {
            val stem = f.name.removeSuffix(".json")
            assertTrue(cases.containsKey(stem), "uncovered request fixture: $stem")
        }
    }

    @Test
    fun requestFixturesMatchSdkSerialization() {
        for ((stem, tc) in requestCases()) {
            val raw = fixturesDir().resolve("$stem.json").readText()
            val expected = testJson.parseToJsonElement(raw)

            val frame =
                JsonRpcRequest(
                    id = JsonPrimitive(tc.id),
                    method = tc.method.wireName,
                    params = tc.params,
                )
            val builtRaw = testJson.encodeToString(JsonRpcRequest.serializer(), frame)
            val built = testJson.parseToJsonElement(builtRaw)

            assertEquals(expected, built, "$stem: built frame differs from fixture")
        }
    }

    @Test
    fun responseFixturesClassifyAsResponses() {
        val fixtures =
            fixturesDir().listFiles { f -> f.name.endsWith(".response.json") }
                ?: fail("no response fixtures found")
        assertTrue(fixtures.isNotEmpty(), "no response fixtures found")
        for (f in fixtures) {
            val frame = classifyIncomingFrame(f.readText(), testJson)
            assertEquals(FrameKind.Response, frame.kind, "${f.name} should classify as a response")
        }
    }
}
