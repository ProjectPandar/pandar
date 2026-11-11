package zip.iptables.pandar.android.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import zip.iptables.pandar.android.data.remote.ws.LiveState
import zip.iptables.pandar.android.domain.model.Agent
import zip.iptables.pandar.android.domain.model.PandarState
import zip.iptables.pandar.android.ui.jobs.JobsRequestState
import zip.iptables.pandar.android.ui.jobs.jobsUiState
import zip.iptables.pandar.android.ui.printerdetail.PrinterDetailRequestState
import zip.iptables.pandar.android.ui.printerdetail.printerDetailUiState
import zip.iptables.pandar.android.ui.printers.PrintersRequestState
import zip.iptables.pandar.android.ui.printers.printersUiState

class PandarUiStateProjectionTest {
    @Test
    fun `projections reject transient state from a replaced session`() {
        val domain = PandarState(sessionGeneration = 2, hasSession = true)
        val agent = Agent("old-agent", "old-tenant", "Old", "online", "created")

        val printers = printersUiState(
            domain,
            LiveState.CONNECTED,
            PrintersRequestState(
                sessionGeneration = 1,
                loading = false,
                agents = listOf(agent),
                error = "old error",
            ),
        )
        val jobs = jobsUiState(
            domain,
            JobsRequestState(
                sessionGeneration = 1,
                loading = false,
                inFlightId = "old-job",
            ),
        )
        val detail = printerDetailUiState(
            domain,
            "old-printer",
            PrinterDetailRequestState(
                sessionGeneration = 1,
                loading = false,
                toast = "old toast",
            ),
        )

        assertTrue(printers.loading)
        assertTrue(printers.agents.isEmpty())
        assertEquals(null, printers.error)
        assertTrue(jobs.loading)
        assertEquals(null, jobs.inFlightId)
        assertTrue(detail.loading)
        assertEquals(null, detail.toast)
    }
}
