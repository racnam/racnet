package org.racnet.android.messages

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class BoardViewModelTest {
    private val models = ViewModelStore()

    @Before
    fun setMainDispatcher() {
        Dispatchers.setMain(StandardTestDispatcher())
    }

    @After
    fun clearModels() {
        models.clear()
        Dispatchers.resetMain()
    }

    private fun provider(create: () -> BoardViewModel): ViewModelProvider =
        ViewModelProvider(models, object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T = create() as T
        })

    private fun model(
        saved: SavedStateHandle = SavedStateHandle(),
        storeEntry: suspend (ULong, ByteArray) -> Unit,
    ): BoardViewModel = provider { BoardViewModel(saved, storeEntry) }[BoardViewModel::class.java]

    @Test
    fun `pending save survives a recreated owner and subscriber without duplicate posts`() = runTest {
        val release = CompletableDeferred<Unit>()
        val saved = SavedStateHandle()
        var writes = 0
        val board = model(saved) { kind, payload ->
            assertEquals(BoardMessage.KIND, kind)
            assertEquals("pending message", payload.toString(Charsets.UTF_8))
            release.await()
            writes++
        }
        val firstStates = mutableListOf<BoardState>()
        val firstSubscriber = backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            board.state.collect { firstStates.add(it) }
        }
        board.updateDraft("pending message")
        board.post()
        board.post()
        runCurrent()
        assertTrue(firstStates.last().sending)
        assertEquals(0, writes)
        firstSubscriber.cancel()

        val recreated = provider { error("retained model must be reused") }[BoardViewModel::class.java]
        assertSame(board, recreated)
        val newStates = mutableListOf<BoardState>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) {
            recreated.state.collect { newStates.add(it) }
        }
        assertTrue(newStates.last().sending)
        assertEquals("pending message", newStates.last().draft)
        recreated.post()
        release.complete(Unit)
        runCurrent()

        assertEquals(1, writes)
        assertFalse(newStates.last().sending)
        assertEquals("", newStates.last().draft)
        assertEquals("", saved.get<String>("draft"))
    }

    @Test
    fun `completion of an older save preserves a newer draft revision`() = runTest {
        val release = CompletableDeferred<Unit>()
        val saved = SavedStateHandle()
        val board = model(saved) { _, _ -> release.await() }
        board.updateDraft("first draft")
        board.post()
        runCurrent()
        board.updateDraft("new draft")
        // Even editing back to the submitted text is a distinct newer draft.
        board.updateDraft("first draft")
        release.complete(Unit)
        runCurrent()

        assertFalse(board.state.value.sending)
        assertEquals("first draft", board.state.value.draft)
        assertEquals("first draft", saved.get<String>("draft"))
    }

    @Test
    fun `failed save retains draft and can be retried`() = runTest {
        var attempts = 0
        val board = model { _, _ ->
            attempts++
            if (attempts == 1) throw IllegalStateException("storage full")
        }
        board.updateDraft("keep this message")
        board.post()
        runCurrent()
        assertFalse(board.state.value.sending)
        assertEquals("keep this message", board.state.value.draft)
        assertTrue(board.state.value.error!!.contains("storage full"))

        board.post()
        runCurrent()
        assertEquals(2, attempts)
        assertEquals("", board.state.value.draft)
        assertNull(board.state.value.error)
    }

    @Test
    fun `a restored draft starts idle without automatically resending`() = runTest {
        var writes = 0
        val board = model(SavedStateHandle(mapOf("draft" to "restored draft"))) { _, _ -> writes++ }
        runCurrent()

        assertEquals("restored draft", board.state.value.draft)
        assertFalse(board.state.value.sending)
        assertEquals(0, writes)
    }

    @Test
    fun `diagnostic entry saves keep the board draft`() = runTest {
        var writes = 0
        val board = model { kind, payload ->
            assertEquals(0uL, kind)
            assertEquals(100, payload.size)
            writes++
        }
        board.updateDraft("unfinished board post")
        board.createTestEntry(100)
        runCurrent()

        assertEquals(1, writes)
        assertFalse(board.state.value.sending)
        assertEquals("unfinished board post", board.state.value.draft)
    }
}
