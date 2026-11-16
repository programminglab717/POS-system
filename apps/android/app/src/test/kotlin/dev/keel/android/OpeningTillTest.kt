package dev.keel.android

import dev.keel.register.Till
import java.lang.reflect.Method
import java.lang.reflect.Proxy
import kotlin.test.Test
import kotlin.test.assertEquals

/**
 * [OpeningTill] passes every [Till] call on. A call it didn't override would fall to the
 * interface's default, which on the register means a feature that silently isn't there (as card
 * payments once were). The compiler gives the class a stub for each default it doesn't override,
 * so this calls each method and sees whether the till underneath gets it.
 */
class OpeningTillTest {
    private fun Method.key() = name + parameterTypes.joinToString(prefix = "(", postfix = ")") { it.simpleName }

    /** A value of [type] to pass: Kotlin checks a parameter that can't be null isn't. */
    private fun valueOf(type: Class<*>): Any? = when {
        type == Int::class.javaPrimitiveType -> 0
        type == Long::class.javaPrimitiveType -> 0L
        type == Boolean::class.javaPrimitiveType -> false
        type == String::class.java -> ""
        type == List::class.java -> emptyList<Any>()
        // A boxed number or flag is a nullable one.
        type.name.startsWith("java.lang.") -> null
        type.isEnum -> type.enumConstants.first()
        type.isSealed -> valueOf(type.permittedSubclasses.first())
        type.isInterface -> Proxy.newProxyInstance(type.classLoader, arrayOf(type)) { _, _, _ -> null }
        else -> type.declaredFields.firstOrNull { it.name == "INSTANCE" }?.get(null)
            ?: type.constructors.first().let { made -> made.newInstance(*made.parameterTypes.map(::valueOf).toTypedArray()) }
    }

    private fun returned(type: Class<*>): Any? = when (type) {
        Int::class.javaPrimitiveType -> 0
        Long::class.javaPrimitiveType -> 0L
        Boolean::class.javaPrimitiveType -> false
        else -> null
    }

    @Test
    fun everyCallIsPassedOn() {
        val got = mutableSetOf<String>()
        val under = Proxy.newProxyInstance(Till::class.java.classLoader, arrayOf(Till::class.java)) { _, method, _ ->
            got += method.key()
            returned(method.returnType)
        } as Till
        val till = OpeningTill { under }
        till.open()
        val calls = Till::class.java.methods.filter { !it.isSynthetic }
        for (method in calls) {
            // A default that refuses throws: what matters is whether the till underneath got the call.
            runCatching { method.invoke(till, *method.parameterTypes.map(::valueOf).toTypedArray()) }
        }
        assertEquals(emptyList(), calls.map { it.key() }.filter { it !in got }, "calls OpeningTill doesn't pass on")
    }
}
