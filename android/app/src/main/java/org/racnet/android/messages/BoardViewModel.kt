package org.racnet.android.messages

import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

data class BoardState(
    val draft: String = "",
    val sending: Boolean = false,
    val error: String? = null,
)

/** Keeps a pending save alive when the activity rotates or its board is hidden. */
class BoardViewModel(
    private val savedState: SavedStateHandle,
    private val storeEntry: suspend (ULong, ByteArray) -> Unit,
) : ViewModel() {
    private val mutableState = MutableStateFlow(BoardState(draft = savedState["draft"] ?: ""))
    val state: StateFlow<BoardState> = mutableState
    private var draftRevision = 0L

    fun updateDraft(text: String) {
        if (text == mutableState.value.draft) return
        draftRevision++
        savedState["draft"] = text
        mutableState.value = mutableState.value.copy(draft = text)
    }

    fun post() {
        val submittedDraft = mutableState.value.draft
        val submittedRevision = draftRevision
        save("Message", BoardMessage.KIND, { BoardMessage.encode(submittedDraft) }) {
            if (draftRevision == submittedRevision) updateDraft("")
        }
    }

    fun createTestEntry(size: Int) {
        save("Entry", 0uL, { ByteArray(size) { (it % 251).toByte() } })
    }

    private fun save(
        label: String,
        kind: ULong,
        payload: () -> ByteArray,
        onSaved: () -> Unit = {},
    ) {
        if (mutableState.value.sending) return
        // Set this before scheduling so repeated taps cannot queue duplicate saves.
        mutableState.value = mutableState.value.copy(sending = true, error = null)
        viewModelScope.launch {
            try {
                storeEntry(kind, payload())
                onSaved()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutableState.value = mutableState.value.copy(
                    error = "$label was not saved: ${e.message.orEmpty()}",
                )
            } finally {
                mutableState.value = mutableState.value.copy(sending = false)
            }
        }
    }
}
