package dev.ferrisidea.mobile

import android.content.Context
import android.content.Intent

/** A release build has no self-test: nothing here confirms digits by itself (SPEC-0326 behaviour 9). */
object SelfTest {
    class Request

    fun request(@Suppress("UNUSED_PARAMETER") intent: Intent?): Request? = null

    @Suppress("UNUSED_PARAMETER")
    suspend fun run(model: AppModel, request: Request, context: Context) = Unit
}
