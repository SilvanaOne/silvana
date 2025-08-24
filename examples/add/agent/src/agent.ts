import { createGrpcTransport } from "@connectrpc/connect-node";
import { createClient } from "@connectrpc/connect";
import {
  CoordinatorService,
  GetJobRequestSchema,
  CompleteJobRequestSchema,
  FailJobRequestSchema,
  GetSequenceStatesRequestSchema,
} from "./proto/silvana/coordinator/v1/coordinator_pb.js";
import { create } from "@bufbuild/protobuf";
import { deserializeTransitionData } from "./transition.js";
import { getState, SequenceState } from "./state.js";

async function agent() {
  console.time("Agent runtime");
  console.log("Agent is running");
  console.log("Agent arguments:", process.argv.length - 2);

  const startTime = Date.now();
  const maxRunTimeMs = 5 * 60 * 1000; // 5 minutes

  // Create gRPC client over TCP (accessible from Docker container)
  const transport = createGrpcTransport({
    baseUrl: "http://host.docker.internal:50051",
  });

  const client = createClient(CoordinatorService, transport);

  // Get session_id from environment (set by coordinator)
  const sessionId = process.env.SESSION_ID;
  if (!sessionId) {
    console.error("SESSION_ID environment variable is required");
    process.exit(1);
  }

  // Job request parameters
  const request = create(GetJobRequestSchema, {
    developer: "AddExampleDev",
    agent: "AddAgent",
    agentMethod: "prove",
    sessionId: sessionId,
  });

  let jobCount = 0;

  try {
    while (Date.now() - startTime < maxRunTimeMs) {
      console.log("Requesting job from coordinator...");

      const response = await client.getJob(request);

      if (response.job) {
        jobCount++;
        console.log(
          `Received job ${jobCount}: ID=${response.job.jobSequence}, job_id=${response.job.jobId}`
        );

        try {
          // Deserialize the job data to get TransitionData with sequence
          console.log("Processing job...");
          console.log(`Job data length: ${response.job.data.length}`);
          console.log(
            `Job data first 20 bytes: ${Array.from(
              response.job.data.slice(0, 20)
            )}`
          );
          const transitionData = deserializeTransitionData(
            Array.from(response.job.data)
          );
          console.log(
            `Job ${response.job.jobSequence} has transition data with sequence: ${transitionData.sequence}`
          );

          // Query sequence states using the sequence from TransitionData
          const sequenceStatesRequest = create(GetSequenceStatesRequestSchema, {
            sessionId: sessionId,
            jobId: response.job.jobId,
            sequence: transitionData.sequence,
          });

          console.log(
            `Querying sequence states for sequence ${transitionData.sequence}...`
          );
          const sequenceStatesResponse = await client.getSequenceStates(
            sequenceStatesRequest
          );

          console.log(
            `Retrieved ${sequenceStatesResponse.states.length} sequence states:`
          );
          sequenceStatesResponse.states.forEach((state, index) => {
            console.log(
              `  State ${index + 1}: sequence=${
                state.sequence
              }, has_state=${!!state.state}, has_data_availability=${!!state.dataAvailability}`
            );
          });

          // Prepare sequence states for getState function
          const sequenceStates: SequenceState[] = [];

          for (const state of sequenceStatesResponse.states) {
            try {
              // Deserialize transition data from each sequence state
              console.log(
                `Processing sequence ${state.sequence}: transitionData length=${
                  state.transitionData.length
                }, first 20 bytes=[${Array.from(
                  state.transitionData.slice(0, 20)
                ).join(",")}]`
              );

              // Sequence 0 is the initial state and has no transition data
              if (Number(state.sequence) === 0) {
                if (state.transitionData.length === 0) {
                  console.log(
                    `Sequence 0 is initial state with no transition data - skipping deserialization`
                  );
                  // Skip sequence 0 as it has no transition data (initial state)
                  continue;
                } else {
                  console.warn(
                    `Sequence 0 unexpectedly has transition data (length: ${state.transitionData.length})`
                  );
                }
              }

              const transition = deserializeTransitionData(
                Array.from(state.transitionData)
              );

              sequenceStates.push({
                sequence: Number(state.sequence),
                transition: transition,
                dataAvailability: state.dataAvailability || undefined,
              });

              console.log(
                `Successfully deserialized sequence ${state.sequence} with transition sequence: ${transition.sequence}`
              );
            } catch (error) {
              console.error(
                `Failed to deserialize sequence ${state.sequence}:`,
                error
              );
              console.error(
                `TransitionData bytes: [${Array.from(state.transitionData).join(
                  ","
                )}]`
              );
              // Continue processing other sequences
            }
          }

          console.log(
            `Processed ${sequenceStates.length} sequence states for getState`
          );

          // Call getState to get the current program state
          try {
            const result = await getState(sequenceStates, client, sessionId);

            if (result) {
              console.log(
                `Successfully retrieved program state and map from sequence states`
              );
              console.log(
                `State available: sum ${result.state.sum.toBigInt()}, Map available: root: ${result.map.root.toBigInt()}`
              );
              // Here you can use result.state and result.map for further processing
            } else {
              console.log(
                `No program state could be retrieved from sequence states`
              );
            }
          } catch (error) {
            console.error(`Failed to get program state:`, error);
          }

          // Simulate job processing for 5 seconds
          await sleep(5000);

          // Complete the job
          const completeRequest = create(CompleteJobRequestSchema, {
            jobId: response.job.jobId,
            sessionId: sessionId,
          });

          console.log(`Completing job ${response.job.jobId}...`);
          const completeResponse = await client.completeJob(completeRequest);

          if (completeResponse.success) {
            console.log(
              `Job ${response.job.jobSequence} completed successfully`
            );
          } else {
            console.error(
              `Failed to complete job ${response.job.jobSequence}: ${completeResponse.message}`
            );
          }
        } catch (error) {
          console.error(
            `Job ${response.job.jobSequence} processing failed:`,
            error
          );

          // Fail the job
          try {
            const failRequest = create(FailJobRequestSchema, {
              jobId: response.job.jobId,
              errorMessage: `Job processing failed: ${error}`,
              sessionId: sessionId,
            });

            console.log(`Failing job ${response.job.jobId}...`);
            const failResponse = await client.failJob(failRequest);

            if (failResponse.success) {
              console.log(
                `Job ${response.job.jobSequence} failed successfully`
              );
            } else {
              console.error(
                `Failed to fail job ${response.job.jobSequence}: ${failResponse.message}`
              );
            }
          } catch (failError) {
            console.error(
              `Failed to call FailJob for ${response.job.jobSequence}:`,
              failError
            );
          }
        }
      } else {
        console.log("No job available - exiting");
        break;
      }

      // Check if we still have time for another job
      const elapsedMs = Date.now() - startTime;
      const remainingMs = maxRunTimeMs - elapsedMs;
      console.log(
        `Elapsed: ${Math.round(elapsedMs / 1000)}s, Remaining: ${Math.round(
          remainingMs / 1000
        )}s`
      );

      if (remainingMs < 10000) {
        // Less than 10 seconds remaining
        console.log("Not enough time remaining for another job - exiting");
        break;
      }
    }
  } catch (error) {
    console.error("gRPC call failed:", error);
  }

  console.log(`Agent processed ${jobCount} jobs`);
  console.timeEnd("Agent runtime");
}

async function sleep(ms: number) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

agent().catch(console.error);
