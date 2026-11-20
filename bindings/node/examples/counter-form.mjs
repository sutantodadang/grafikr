import { Client, Node, State, Window } from "@grafikr/ui";

const client = await Client.start();
await client.ready;
const count = await State.create(client, { type: "i64", value: 0n });
const window = await Window.create(client, "Grafikr counter and form", 520, 320);
const root = await Node.create(client, window, "Column", { gap: 12, padding: 16 });
const countLabel = await Node.create(client, window, "Text", { text: "Count: 0", size: 24 });
const increment = await Node.create(client, window, "Button", { label: "Increment" });
const textField = await Node.create(client, window, "TextField", { text: "Grafikr 雪", width: 320 });
const preview = await Node.create(client, window, "Text", { text: "Grafikr 雪" });

await root.attach(window);
await countLabel.attach(root);
await increment.attach(root);
await textField.attach(root);
await preview.attach(root);

const subscriptions = await Promise.all([
  client.on(increment.handle, "click", async () => {
    const next = (await count.get()) + 1n;
    await count.set({ type: "i64", value: next });
    await countLabel.update({ text: `Count: ${next}` });
  }),
  client.on(textField.handle, "change", async event => {
    if (typeof event.payload?.text === "string") await preview.update({ text: event.payload.text });
  }),
]);

console.log("Window is running. Press Ctrl+C to close it.");
await new Promise(resolve => process.once("SIGINT", resolve));
for (const subscription of subscriptions) await subscription.close();
await count.close();
await window.close();
await client.close();
