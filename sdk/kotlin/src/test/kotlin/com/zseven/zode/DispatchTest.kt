package com.zseven.zode

import java.util.Collections
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlin.test.Test
import kotlin.test.assertEquals

class DispatchTest {
    // Child waits for both requests, emits a notification, then answers in
    // reverse arrival order echoing each request's own tag, proving the pending
    // map routes by id rather than by arrival order.
    private val outOfOrderBody =
        """
        read line1
        read line2
        id1=${'$'}(printf '%s' "${'$'}line1" | sed 's/.*"id":\([0-9]*\).*/\1/')
        tag1=${'$'}(printf '%s' "${'$'}line1" | sed 's/.*"tag":"\([^"]*\)".*/\1/')
        id2=${'$'}(printf '%s' "${'$'}line2" | sed 's/.*"id":\([0-9]*\).*/\1/')
        tag2=${'$'}(printf '%s' "${'$'}line2" | sed 's/.*"tag":"\([^"]*\)".*/\1/')
        printf '{"jsonrpc":"2.0","method":"turn/started","params":{"turnId":"t"}}\n'
        printf '{"jsonrpc":"2.0","id":%s,"result":{"tag":"%s"}}\n' "${'$'}id2" "${'$'}tag2"
        printf '{"jsonrpc":"2.0","id":%s,"result":{"tag":"%s"}}\n' "${'$'}id1" "${'$'}tag1"
        """.trimIndent()

    @Test
    fun dispatchesNotificationsWhileResolvingOutOfOrder() {
        val client = ZodeClient(binary = scriptedChild(outOfOrderBody))
        try {
            val notes = Collections.synchronizedList(mutableListOf<String>())
            client.onNotification { method, _ -> notes.add(method) }

            val results = arrayOfNulls<String>(2)
            val tags = listOf("one", "two")
            val threads =
                tags.mapIndexed { i, tag ->
                    Thread {
                        val res = client.request("req", buildJsonObject { put("tag", tag) })
                        results[i] = res.jsonObject["tag"]?.jsonPrimitive?.content
                    }.apply { start() }
                }
            threads.forEach { it.join(5000) }

            assertEquals("one", results[0])
            assertEquals("two", results[1])
            assertEquals(listOf("turn/started"), notes.toList())
        } finally {
            client.close()
        }
    }

    // Child asks for approval then echoes the received decision into the
    // pending request's result so the test can assert it.
    private val approvalBody =
        """
        read request
        printf '{"jsonrpc":"2.0","id":"approval-1","method":"approval/request","params":{"approvalId":"a1","kind":"command","summary":"run"}}\n'
        read approval
        case "${'$'}approval" in
          *'"decision":"allowAlways"'*) printf '{"jsonrpc":"2.0","id":1,"result":{"decision":"allowAlways"}}\n';;
          *'"decision":"allow"'*) printf '{"jsonrpc":"2.0","id":1,"result":{"decision":"allow"}}\n';;
          *'"decision":"deny"'*) printf '{"jsonrpc":"2.0","id":1,"result":{"decision":"deny"}}\n';;
          *) exit 2;;
        esac
        """.trimIndent()

    private fun runApproval(
        register: Boolean,
        handler: (ApprovalRequestParams) -> ApprovalDecision,
    ): String {
        val client = ZodeClient(binary = scriptedChild(approvalBody))
        try {
            if (register) client.onApprovalRequest(handler)
            val res = client.request("test", buildJsonObject {})
            return res.jsonObject["decision"]!!.jsonPrimitive.content
        } finally {
            client.close()
        }
    }

    @Test
    fun approvalAllow() {
        val got =
            runApproval(register = true) { p ->
                assertEquals("a1", p.approvalId)
                ApprovalDecision.Allow
            }
        assertEquals("allow", got)
    }

    @Test
    fun approvalDeny() {
        val got = runApproval(register = true) { ApprovalDecision.Deny }
        assertEquals("deny", got)
    }

    @Test
    fun approvalMissingHandlerDenies() {
        val got = runApproval(register = false) { ApprovalDecision.Allow }
        assertEquals("deny", got)
    }

    @Test
    fun approvalThrowingHandlerDenies() {
        val got = runApproval(register = true) { throw RuntimeException("boom") }
        assertEquals("deny", got)
    }

    @Test
    fun initializeIncludesExplicitApprovalPolicy() {
        val child =
            scriptedChild(
                """
                read request
                case "${'$'}request" in
                  *'"jsonrpc":"2.0"'*'"approvalPolicy":"auto"'*) printf '{"jsonrpc":"2.0","id":1,"result":{"ok":true}}\n';;
                  *) exit 2;;
                esac
                """.trimIndent(),
            )
        val client = ZodeClient(binary = child)
        try {
            val res: JsonElement = client.initialize("test", "1", "auto")
            assertEquals("true", res.jsonObject["ok"]!!.jsonPrimitive.content)
        } finally {
            client.close()
        }
    }

    @Test
    fun initializeOmitsApprovalPolicyByDefault() {
        val child =
            scriptedChild(
                """
                read request
                case "${'$'}request" in
                  *'"approvalPolicy"'*) exit 2;;
                  *'"jsonrpc":"2.0"'*) printf '{"jsonrpc":"2.0","id":1,"result":{"ok":true}}\n';;
                  *) exit 2;;
                esac
                """.trimIndent(),
            )
        val client = ZodeClient(binary = child)
        try {
            val res = client.initialize("test", "1", null)
            assertEquals("true", res.jsonObject["ok"]!!.jsonPrimitive.content)
        } finally {
            client.close()
        }
    }
}
