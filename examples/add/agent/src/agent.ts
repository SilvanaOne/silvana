import {
  getJob,
  completeJob,
  failJob,
  getSequenceStates,
  submitProof,
  submitState,
  getProof,
  info,
  error,
} from "@silvana-one/agent";
import { deserializeTransitionData } from "./transition.js";
import { getStateAndProof, SequenceState, merge } from "./state.js";
import { serializeProofAndState, serializeState } from "./proof.js";
import { settle } from "./settle.js";

async function agent() {
  console.time("Agent runtime");
  info("Agent is running");

  const startTime = Date.now();
  const maxRunTimeMs = 500 * 1000; // 500 seconds (8.33 minutes) - absolute maximum runtime
  const stopAcceptingJobsMs = 400 * 1000; // 400 seconds (6.67 minutes) - stop accepting new jobs

  let jobCount = 0;

  try {
    while (Date.now() - startTime < maxRunTimeMs) {
      info("Requesting job from coordinator...");

      const response = await getJob();

      if (response.job) {
        jobCount++;
        info(
          `Received job ${jobCount}: ID=${response.job.jobSequence}, job_id=${response.job.jobId}`
        );

        try {
          // Check if this is a settle job by app_instance_method
          if (response.job.appInstanceMethod === "settle") {
            console.log("⚖️ SETTLE JOB DETECTED");

            // Verify chain is provided for settle jobs
            if (!response.job.chain) {
              throw new Error(
                "Settlement job received without chain parameter. Chain is required for settle jobs."
              );
            }

            // Print all job details
            console.log("=== SETTLE JOB DETAILS ===");
            console.log(`Job Sequence: ${response.job.jobSequence}`);
            console.log(`Job ID: ${response.job.jobId}`);
            console.log(`Description: ${response.job.description || "none"}`);
            console.log(`Settlement Chain: ${response.job.chain}`);
            console.log(`Developer: ${response.job.developer}`);
            console.log(`Agent: ${response.job.agent}`);
            console.log(`Agent Method: ${response.job.agentMethod}`);
            console.log(`App: ${response.job.app}`);
            console.log(`App Instance: ${response.job.appInstance}`);
            console.log(
              `App Instance Method: ${response.job.appInstanceMethod}`
            );
            console.log(`Sequences: [${response.job.sequences.join(", ")}]`);
            console.log(`Attempts: ${response.job.attempts}`);
            console.log(`Created At: ${response.job.createdAt}`);
            console.log(`Updated At: ${response.job.updatedAt}`);
            console.log(`Data Length: ${response.job.data.length}`);
            console.log(
              `Data First 20 bytes: ${Array.from(
                response.job.data.slice(0, 20)
              )}`
            );
            console.log("==========================");

            // Get the block proof for settlement
            const blockNumber = response.job.blockNumber ?? 1n;
            console.log(`\nFetching block proof for block ${blockNumber}...`);

            try {
              // Settle the block proof on-chain
              console.log("\n🔐 Starting settlement process...");
              const settlementStartTime = Date.now();

              try {
                // Call the settle function which will handle everything:
                // - Fetch metadata and get admin/contract info
                // - Get admin private key from secrets
                // - Submit proof to Mina blockchain
                // - Update settlement tx hash on Sui blockchain
                await settle({
                  settlementChain: response.job.chain,
                  // privateKey and contractAddress are optional
                  // They will be fetched from metadata if not provided
                });

                const settlementTimeMs = Date.now() - settlementStartTime;
                console.log(
                  `\n✅ Settlement completed successfully in ${settlementTimeMs}ms`
                );

                console.log("Block proofs have been settled on-chain!");

                // Complete the job successfully
                console.log(`\nCompleting settle job ${response.job.jobId}...`);
                const completeResponse = await completeJob();
                if (completeResponse.success) {
                  console.log(
                    `✅ Settle job completed successfully: ${completeResponse.message}`
                  );
                } else {
                  error(
                    `Failed to complete settle job: ${completeResponse.message}`
                  );
                }
              } catch (settleError) {
                throw settleError; // Re-throw to be caught by outer catch
              }
            } catch (err) {
              error(`\n❌ Failed to settle block: ${err}`);

              // Fail the job
              console.log(
                `Failing job ${response.job.jobId} due to settlement error...`
              );
              await failJob(`Settlement failed: ${err}`);
              console.log("Job marked as failed");
              continue;
            }

            console.log("Settle job processing complete");
            continue; // Skip to next job to avoid duplicate completeJob call
          } else if (response.job.appInstanceMethod === "merge") {
            console.log("🔀 MERGE JOB DETECTED");
            // Print all job details
            console.log("=== JOB DETAILS ===");
            console.log(`Job Sequence: ${response.job.jobSequence}`);
            console.log(`Job ID: ${response.job.jobId}`);
            console.log(`Description: ${response.job.description || "none"}`);
            console.log(`Developer: ${response.job.developer}`);
            console.log(`Agent: ${response.job.agent}`);
            console.log(`Agent Method: ${response.job.agentMethod}`);
            console.log(`App: ${response.job.app}`);
            console.log(`App Instance: ${response.job.appInstance}`);
            console.log(
              `App Instance Method: ${response.job.appInstanceMethod}`
            );
            console.log(`Sequences: [${response.job.sequences.join(", ")}]`);
            console.log(`Block Number: ${response.job.blockNumber}`);
            console.log(`Attempts: ${response.job.attempts}`);
            console.log(`Created At: ${response.job.createdAt}`);
            console.log(`Updated At: ${response.job.updatedAt}`);
            console.log(`Data Length: ${response.job.data.length}`);
            console.log(
              `Data First 20 bytes: ${Array.from(
                response.job.data.slice(0, 20)
              )}`
            );
            console.log("==================");

            // Get merge data directly from Job fields
            const blockNumber = response.job.blockNumber!;
            const sequences1 = response.job.sequences1;
            const sequences2 = response.job.sequences2;

            console.log(
              `Job ${response.job.jobSequence} is a merge job for block ${blockNumber}`
            );

            // Display the merge data
            console.log(`ProofMergeData:`);
            console.log(`  block_number: ${blockNumber}`);
            console.log(`  sequences1: [${sequences1.join(", ")}]`);
            console.log(`  sequences2: [${sequences2.join(", ")}]`);
            console.log(
              `  Total sequences to merge: ${
                sequences1.length + sequences2.length
              }`
            );

            // Fetch the two proofs to merge
            console.log("Fetching proofs to merge...");

            try {
              // Fetch first proof
              console.log(
                `Fetching proof 1: sequences ${sequences1.join(", ")}`
              );
              const proof1Response = await getProof(blockNumber, sequences1);
              if (!proof1Response.success || !proof1Response.proof) {
                throw new Error(
                  `Failed to fetch proof 1: ${
                    proof1Response.message || "Unknown error"
                  }`
                );
              }
              console.log(
                `Successfully fetched proof 1 (${proof1Response.proof.length} chars)`
              );

              // Extract just the proof part from the serialized data
              const proof1Data = JSON.parse(proof1Response.proof);
              const proof1Only = proof1Data.proof;

              // Fetch second proof
              console.log(
                `Fetching proof 2: sequences ${sequences2.join(", ")}`
              );
              const proof2Response = await getProof(blockNumber, sequences2);
              if (!proof2Response.success || !proof2Response.proof) {
                throw new Error(
                  `Failed to fetch proof 2: ${
                    proof2Response.message || "Unknown error"
                  }`
                );
              }
              console.log(
                `Successfully fetched proof 2 (${proof2Response.proof.length} chars)`
              );

              // Extract just the proof part from the serialized data
              const proof2Data = JSON.parse(proof2Response.proof);
              const proof2Only = proof2Data.proof;

              // Merge the proofs
              console.log("Starting proof merge...");
              const mergeStartTime = Date.now();
              const mergedProof = await merge({
                blockNumber: BigInt(blockNumber),
                proof1Serialized: proof1Only,
                proof2Serialized: proof2Only,
                sequences1: sequences1,
                sequences2: sequences2,
              });
              const mergeTimeMs = Date.now() - mergeStartTime;
              console.log(
                `Merge completed! Merged proof size: ${mergedProof.length} chars`
              );

              // Combine and sort all sequences for submission
              const allSequences = [...sequences1, ...sequences2].sort(
                (a, b) => Number(a) - Number(b)
              );

              console.log(
                `Submitting merged proof for sequences: ${allSequences.join(
                  ", "
                )}`
              );

              // Submit the merged proof
              const submitProofResponse = await submitProof(
                blockNumber,
                allSequences,
                mergedProof,
                BigInt(mergeTimeMs),
                sequences1,
                sequences2
              );
              console.log(
                `Merged proof submitted successfully! TX: ${submitProofResponse.txHash}, DA: ${submitProofResponse.daHash}`
              );
            } catch (error) {
              console.error(`Failed to merge proofs: ${error}`);
              // Fail the job instead of completing it
              console.log(
                `Failing job ${response.job.jobId} due to merge error...`
              );
              await failJob(`Merge failed: ${error}`);
              console.log(`Job ${jobCount} failed due to merge error`);
              continue; // Skip to next job without marking as complete
            }
          } else {
            // Default to prove job processing
            console.log("⚡ PROVE JOB DETECTED");
            const transitionData = deserializeTransitionData(
              Array.from(response.job.data)
            );
            console.log("=== JOB DETAILS ===");
            console.log(`Job Sequence: ${response.job.jobSequence}`);
            console.log(`Job ID: ${response.job.jobId}`);
            console.log(`Description: ${response.job.description || "none"}`);
            console.log(`Developer: ${response.job.developer}`);
            console.log(`Agent: ${response.job.agent}`);
            console.log(`Agent Method: ${response.job.agentMethod}`);
            console.log(`App: ${response.job.app}`);
            console.log(`App Instance: ${response.job.appInstance}`);
            console.log(
              `App Instance Method: ${response.job.appInstanceMethod}`
            );
            console.log(`Sequences: [${response.job.sequences.join(", ")}]`);
            console.log(`Block Number: ${response.job.blockNumber}`);
            console.log(`Attempts: ${response.job.attempts}`);
            console.log(`Created At: ${response.job.createdAt}`);
            console.log(`Updated At: ${response.job.updatedAt}`);
            console.log(`Data Length: ${response.job.data.length}`);
            console.log(
              `Data First 20 bytes: ${Array.from(
                response.job.data.slice(0, 20)
              )}`
            );
            console.log("==================");
            console.log(
              `Job ${response.job.jobSequence} has transition data with sequence: ${transitionData.sequence}`
            );

            // Query sequence states using the sequence from TransitionData
            console.log(
              `Querying sequence states for sequence ${transitionData.sequence}...`
            );
            const sequenceStatesResponse = await getSequenceStates(
              BigInt(transitionData.sequence)
            );

            console.log(
              `Retrieved ${sequenceStatesResponse.states.length} sequence states:`
            );
            sequenceStatesResponse.states.forEach((state, index) => {
              console.log(
                `  State ${index + 1}: sequence=${
                  state.sequence
                }, has_state=${!!state.state}, has_data_availability=${
                  state.dataAvailability ?? "none"
                }`
              );
            });

            // Prepare sequence states for getState function
            const sequenceStates: SequenceState[] = [];

            for (const state of sequenceStatesResponse.states) {
              try {
                // Deserialize transition data from each sequence state
                console.log(
                  `Processing sequence ${
                    state.sequence
                  }: transitionData length=${
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
              } catch (err) {
                error(`Failed to deserialize sequence ${state.sequence}:`, err);
                error(
                  `TransitionData bytes: [${Array.from(
                    state.transitionData
                  ).join(",")}]`
                );
                // Continue processing other sequences
              }
            }

            console.log(
              `Processed ${sequenceStates.length} sequence states for getState`
            );

            // Call getStateAndProof to get the current program state and proof
            // Note: Removed inner try-catch so errors propagate to outer handler
            const startTime = Date.now();
            const result = await getStateAndProof({
              sequenceStates,
              sequence: transitionData.sequence,
              blockNumber: transitionData.block_number,
            });
            const endTime = Date.now();
            const cpuTimeMs = endTime - startTime;

            if (result) {
              console.log(
                `Successfully retrieved program state and map from sequence states`
              );
              console.log(
                `State available: sum ${result.state.sum.toBigInt()}, Map available: root: ${result.map.root.toBigInt()}`
              );
              console.log(`Total processing time: ${cpuTimeMs}ms`);

              if (result.proof) {
                console.log(
                  `Proof generated for sequence ${transitionData.sequence}`
                );

                // Serialize proof and state separately
                const serializedProofAndState = serializeProofAndState(
                  result.proof,
                  result.state,
                  result.map
                );
                const serializedStateOnly = serializeState(
                  result.state,
                  result.map
                );

                console.log(`Serialized proof and state for submission`);
                console.log(
                  `Proof size: ${serializedProofAndState.length} chars`
                );
                console.log(`State size: ${serializedStateOnly.length} chars`);

                console.log(
                  `Submitting proof and state sequentially for sequence ${transitionData.sequence}...`
                );

                // Submit proof first and wait for completion
                try {
                  console.log(
                    `Submitting proof for sequence ${transitionData.sequence}...`
                  );
                  const submitProofResponse = await submitProof(
                    BigInt(transitionData.block_number),
                    [transitionData.sequence],
                    serializedProofAndState,
                    BigInt(cpuTimeMs)
                  );
                  console.log(
                    `Proof submitted successfully for sequence ${transitionData.sequence}`
                  );

                  // Then submit state after proof is done
                  console.log(
                    `Submitting state for sequence ${transitionData.sequence}...`
                  );
                  const submitStateResponse = await submitState(
                    transitionData.sequence,
                    undefined,
                    serializedStateOnly
                  );
                  console.log(
                    `State submitted successfully for sequence ${transitionData.sequence}`
                  );

                  console.log(
                    `Both proof and state submitted successfully for sequence ${transitionData.sequence}`
                  );
                  console.log(
                    `Proof transaction hash: ${submitProofResponse.txHash}`
                  );
                  console.log(
                    `Proof data availability hash: ${submitProofResponse.daHash}`
                  );
                  console.log(
                    `State transaction hash: ${submitStateResponse.txHash}`
                  );
                  if (submitStateResponse.daHash) {
                    console.log(
                      `State data availability hash: ${submitStateResponse.daHash}`
                    );
                  }
                } catch (submitError) {
                  console.error(
                    `Failed to submit proof and/or state for sequence ${transitionData.sequence}:`,
                    submitError
                  );
                  // Re-throw with a more descriptive error for the job failure
                  const errorMessage =
                    submitError instanceof Error
                      ? submitError.message
                      : String(submitError);
                  throw new Error(
                    `Failed to submit proof/state for sequence ${transitionData.sequence}: ${errorMessage}`
                  );
                }
              } else {
                console.log(
                  `No proof generated for sequence ${transitionData.sequence}`
                );
              }

              // Here you can use result.state, result.map, and result.proof for further processing
            } else {
              console.log(
                `No program state could be retrieved from sequence states`
              );
            }
          } // Close the else block for prove job processing

          // Complete the job
          console.log(`Completing job ${response.job.jobId}...`);
          const completeResponse = await completeJob();

          if (completeResponse.success) {
            console.log(
              `Job ${response.job.jobSequence} completed successfully`
            );
          } else {
            console.error(
              `Failed to complete job ${response.job.jobSequence}: ${completeResponse.message}`
            );
          }
        } catch (err) {
          console.error(
            `Job ${response.job.jobSequence} processing failed:`,
            err
          );

          // Fail the job
          try {
            // Extract a meaningful error message
            let errorMessage = "Job processing failed";
            if (err instanceof Error) {
              errorMessage = `Job processing failed: ${err.message}`;
            } else if (typeof err === "string") {
              errorMessage = `Job processing failed: ${err}`;
            } else if (err && typeof err === "object" && "message" in err) {
              errorMessage = `Job processing failed: ${(error as any).message}`;
            } else {
              errorMessage = `Job processing failed: ${JSON.stringify(err)}`;
            }

            console.log(`Failing job ${response.job.jobId}...`);
            const failResponse = await failJob(errorMessage);

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

      // Check if we should continue accepting new jobs
      const elapsedMs = Date.now() - startTime;
      const remainingMs = maxRunTimeMs - elapsedMs;
      const remainingForNewJobsMs = stopAcceptingJobsMs - elapsedMs;

      console.log(
        `Elapsed: ${Math.round(
          elapsedMs / 1000
        )}s, Max runtime remaining: ${Math.round(
          remainingMs / 1000
        )}s, New jobs cutoff in: ${Math.round(
          Math.max(0, remainingForNewJobsMs / 1000)
        )}s`
      );

      // Stop accepting new jobs after 400 seconds
      if (elapsedMs >= stopAcceptingJobsMs) {
        console.log("Reached 400 second cutoff - not accepting new jobs");
        break;
      }

      // Also check absolute maximum runtime
      if (remainingMs < 10000) {
        // Less than 10 seconds remaining
        console.log("Not enough time remaining for another job - exiting");
        break;
      }
    }
  } catch (err) {
    error("gRPC call failed:", err);
  }

  info(`Agent processed ${jobCount} jobs`);
  console.timeEnd("Agent runtime");
}

agent().catch(console.error);
