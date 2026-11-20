# Grafikr Java and Kotlin SDK

Java 21 client with a Kotlin consumer example. Uses Gson 2.13.2 and the included
Gradle project. Pass an absolute path to an explicitly installed host; process
startup uses `ProcessBuilder`, JSON frames are bounded, responses are
correlated, and callback exceptions do not escape the event executor.

`CollectionProvider.create(node, rowCount, rowHeight, columns...)` subscribes
before publishing collection metadata. Call `nextRequest(timeout, unit)` to
read a visible range, answer it with `provide(request, rows)`, and call
`update(...)` when metadata changes. Closing the provider unsubscribes and
wakes a waiting read with `null`.
