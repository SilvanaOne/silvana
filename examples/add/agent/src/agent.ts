async function agent() {
  console.time("Agent runtime");
  console.log("Agent is running");
  console.log("Agent arguments:", process.argv.length - 2);
  await sleep(10000);
  console.timeEnd("Agent runtime");
}

async function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

agent().catch(console.error);
