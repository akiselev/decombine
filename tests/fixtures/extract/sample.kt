// Fixture: functions, methods, lambdas, comments, small bodies.

class Basket(private val prices: Map<String, Int>) {
    fun total(items: List<String>): Int {
        // sum known items, ignore unknown ones
        var sum = 0
        for (item in items) {
            val price = prices[item] ?: continue
            sum += price
        }
        return sum
    }
}

fun describe(items: List<String>): String {
    val cleaned = items.map { item ->
        val trimmed = item.trim()
        val lowered = trimmed.lowercase()
        lowered.replaceFirstChar { c -> c.uppercase() }
    }
    return cleaned.joinToString(separator = ", ")
}

fun tiny(): Int = 1
