package io.unom.punktfunk.kit.link

import io.unom.punktfunk.kit.security.KnownHost
import java.io.File
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * **The cross-language contract.** `clients/shared/start-screen-vectors.json` is consumed verbatim
 * by the Rust, Swift and Kotlin suites, so the three resolvers cannot send three clients to three
 * different screens. Any new case belongs in that file, not in this one.
 */
class StartScreenVectorTest {
    private val vectors: JSONObject by lazy {
        // Gradle runs a unit test with the module directory as its working directory, so the
        // shared file is two levels up (clients/android/kit → clients/shared). Resolved rather
        // than copied: a copy would be a fourth contract, free to go stale.
        val file = File("../../shared/start-screen-vectors.json")
        assertTrue(
            "the shared vector file must be reachable at ${file.absolutePath}",
            file.isFile,
        )
        JSONObject(file.readText())
    }

    /**
     * A vector host carries only what the policy reads. `pinned` is whether the record holds a
     * fingerprint at all — each store spells one differently, so the file only asks if there is
     * one.
     */
    private fun host(spec: JSONObject, index: Int) = KnownHost(
        address = "10.0.0.${index + 1}",
        port = 9777,
        name = spec.getString("name"),
        fpHex = if (spec.getBoolean("pinned")) "ab".repeat(32) else "",
        paired = spec.getBoolean("paired"),
        id = spec.getString("id"),
    )

    @Test
    fun everySharedVectorAgrees() {
        val cases = vectors.getJSONArray("cases")
        assertTrue("the vector file is the contract", cases.length() >= 10)
        for (i in 0 until cases.length()) {
            val case = cases.getJSONObject(i)
            val name = case.getString("name")
            val specs = case.getJSONArray("hosts")
            val hosts = (0 until specs.length()).map { host(specs.getJSONObject(it), it) }
            val pointer = if (case.has("default_host")) case.getString("default_host") else null

            val resolved = StartScreen.defaultHost(pointer, hosts)
            val expect = case.getJSONObject("expect")
            assertEquals(
                "$name source",
                expect.getString("source"),
                resolved.source.name.lowercase(),
            )
            assertEquals(
                "$name host_index",
                if (expect.has("host_index")) expect.getInt("host_index") else null,
                resolved.host?.let { h -> hosts.indexOfFirst { it.id == h.id } },
            )

            val start = StartScreen.resolve(case.getString("start_in"), pointer, hosts)
            val got = when (start) {
                is StartScreen.Hosts -> "hosts"
                is StartScreen.Library -> "library"
                is StartScreen.Stream -> "stream"
            }
            assertEquals("$name start", expect.getString("start"), got)
        }
    }

    /**
     * The Apple client writes `UUID.uuidString`, which is uppercase, into the same key name the
     * Rust client mints lowercase into. Either spelling must resolve.
     */
    @Test
    fun theDefaultHostPointerIsCaseInsensitive() {
        val hosts = listOf(
            KnownHost("10.0.0.5", 9777, "Desk", "ab".repeat(32), true, id = "a1b2-c3"),
            KnownHost("10.0.0.6", 9777, "Couch", "cd".repeat(32), true, id = "d4e5-f6"),
        )
        for (spelling in listOf("a1b2-c3", "A1B2-C3")) {
            val resolved = StartScreen.defaultHost(spelling, hosts)
            assertEquals(spelling, "Desk", resolved.host?.name)
            assertEquals(DefaultHostSource.EXPLICIT, resolved.source)
        }
    }

    /** An unpaired store has nothing to land on, so every setting opens the list. */
    @Test
    fun anUnpairedStoreAlwaysOpensTheList() {
        val hosts = listOf(KnownHost("10.0.0.5", 9777, "Desk", "", false, id = "a"))
        for (value in StartIn.entries) {
            assertEquals(
                "${value.stored} landed somewhere with nothing to land on",
                StartScreen.Hosts,
                StartScreen.resolve(value.stored, null, hosts),
            )
        }
        assertEquals(StartIn.HOSTS, StartIn.parse("shelf"))
        assertEquals(StartIn.HOSTS, StartIn.parse(null))
    }
}
