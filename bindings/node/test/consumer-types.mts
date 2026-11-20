import { Client, State, type Handle } from "@grafikr/ui";

async function consumer(): Promise<void> {
  const client = await Client.start();
  const state = await State.create(client, { type: "i64", value: 1n });
  const value: bigint = await state.get();
  const handle: Handle<"state"> = state.handle;
  void value;
  void handle;
  await client.close();
}

void consumer;
