package zip.iptables.pandar.android.data.settings

import org.junit.Assert.assertEquals
import org.junit.Test
import zip.iptables.pandar.android.data.remote.HubSession

class SettingsMappingTest {

    @Test fun rejected_session_only_clears_the_matching_token_identity() {
        val current = SettingsSnapshot(
            hubBaseUrl = "https://hub.example",
            tenantId = "tenant-1",
            accessToken = "token-2",
            tokenExpiresAtEpochMillis = 2L,
        )
        val rejected = HubSession.create("https://hub.example", "tenant-1", "token-1")!!
        assertEquals(current, current.clearSessionIfMatches(rejected))

        val matching = HubSession.create("https://hub.example", "tenant-1", "token-2")!!
        assertEquals(
            current.copy(accessToken = null, tokenExpiresAtEpochMillis = null),
            current.clearSessionIfMatches(matching),
        )
    }
}
