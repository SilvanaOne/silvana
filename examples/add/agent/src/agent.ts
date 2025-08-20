import { createGrpcTransport } from "@connectrpc/connect-node";
import { createClient } from "@connectrpc/connect";
import {
  CoordinatorService,
  GetJobRequestSchema,
  CompleteJobRequestSchema,
  FailJobRequestSchema,
} from "./proto/silvana/coordinator/v1/coordinator_pb.js";
import { create } from "@bufbuild/protobuf";

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
        console.log(`Received job ${jobCount}: ID=${response.job.jobSequence}, job_id=${response.job.jobId}`);
        
        try {
          // Simulate job processing for 5 seconds
          console.log("Processing job...");
          await sleep(5000);
          
          // Complete the job
          const completeRequest = create(CompleteJobRequestSchema, {
            jobId: response.job.jobId,
            sessionId: sessionId,
          });
          
          console.log(`Completing job ${response.job.jobId}...`);
          const completeResponse = await client.completeJob(completeRequest);
          
          if (completeResponse.success) {
            console.log(`Job ${response.job.jobSequence} completed successfully`);
          } else {
            console.error(`Failed to complete job ${response.job.jobSequence}: ${completeResponse.message}`);
          }
        } catch (error) {
          console.error(`Job ${response.job.jobSequence} processing failed:`, error);
          
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
              console.log(`Job ${response.job.jobSequence} failed successfully`);
            } else {
              console.error(`Failed to fail job ${response.job.jobSequence}: ${failResponse.message}`);
            }
          } catch (failError) {
            console.error(`Failed to call FailJob for ${response.job.jobSequence}:`, failError);
          }
        }
      } else {
        console.log("No job available - exiting");
        break;
      }

      // Check if we still have time for another job
      const elapsedMs = Date.now() - startTime;
      const remainingMs = maxRunTimeMs - elapsedMs;
      console.log(`Elapsed: ${Math.round(elapsedMs/1000)}s, Remaining: ${Math.round(remainingMs/1000)}s`);
      
      if (remainingMs < 10000) { // Less than 10 seconds remaining
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
