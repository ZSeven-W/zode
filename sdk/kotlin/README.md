# Zode Kotlin/JVM SDK

Kotlin/JVM SDK for `zode server` stdio JSON-RPC.

## Install

This directory is a standalone Gradle project.

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

## Test

```sh
cd sdk/kotlin
gradle test
```

This repository's default SDK test script skips Kotlin when Gradle is not
installed.
