# Zode Kotlin/JVM SDK

Kotlin/JVM SDK for `zode server` stdio JSON-RPC.

## Install

The SDK is published to GitHub Packages as
`com.zseven.zode:zode-sdk:0.2.0-beta.2`. Configure the repository and
credentials with a token that has `read:packages`:

```properties
# ~/.gradle/gradle.properties
gpr.user=YOUR_GITHUB_USERNAME
gpr.key=YOUR_GITHUB_TOKEN
```

```kotlin
repositories {
    maven {
        url = uri("https://maven.pkg.github.com/zseven-w/zode")
        credentials {
            username = providers.gradleProperty("gpr.user").orNull
            password = providers.gradleProperty("gpr.key").orNull
        }
    }
}

dependencies {
    implementation("com.zseven.zode:zode-sdk:0.2.0-beta.2")
}
```

This directory is also a standalone Gradle project.

```sh
cd sdk/kotlin
gradle test
```

If you use a Gradle wrapper in your environment, `./gradlew test` works too.

## Usage

`zode` must be on `PATH`, or construct `ZodeClient(binary = "/absolute/path/to/zode")`.

```kotlin
import com.zseven.zode.ZodeClient
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

fun main() {
    ZodeClient().use { client ->
        val init = client.initialize("example", "0.1.0")
        println(init)

        val command = buildJsonObject {
            putJsonArray("command") {
                add("sh")
                add("-c")
                add("printf hi")
            }
        }
        val result = client.request(ProtocolMethod.CommandExec, command)
        println(result)
    }
}
```

Use `client.request(ProtocolMethod.CommandExec, params)` for stable zode
methods, or pass a raw string when you intentionally need low-level JSON-RPC.
Every supported method's params, result shape, and enum name are documented in
the [SDK method reference](../README.md#method-reference).

## Streaming turns and approvals

Register handlers before starting a turn. Pass `approvalPolicy = "auto"` (or
`"prompt"` with an approval handler) so side-effecting work runs — the default
`readOnly` denies it.

```kotlin
import com.zseven.zode.ApprovalDecision
import com.zseven.zode.ProtocolMethod
import com.zseven.zode.ZodeClient
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

fun main() {
    ZodeClient().use { client ->
        client.onNotification { method, params ->
            if (method == "item/agentMessage/delta") {
                print(params?.jsonObject?.get("delta")?.jsonPrimitive?.content ?: "")
            }
        }
        client.onApprovalRequest { params ->
            System.err.println("approve ${params.kind}: ${params.summary}")
            ApprovalDecision.Allow // Allow | AllowAlways | Deny
        }

        client.initialize("example", "0.1.0", approvalPolicy = "auto")
        val thread = client.request(ProtocolMethod.ThreadStart, buildJsonObject { })
        val threadId = thread.jsonObject["thread"]!!.jsonObject["id"]!!.jsonPrimitive.content
        client.request(
            ProtocolMethod.TurnStart,
            buildJsonObject {
                put("threadId", threadId)
                put("input", "list the repo files")
            },
        )
    }
}
```

`onNotification` receives `(method, params)` where `params` is a nullable
`JsonElement`. `onApprovalRequest` returns an `ApprovalDecision`; an
unregistered or throwing handler denies.

## Version

`0.2.0-beta.2`.

## Test

```sh
cd sdk/kotlin
gradle test
```

This repository's default SDK test script skips Kotlin when Gradle is not
installed.
