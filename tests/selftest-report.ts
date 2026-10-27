// Builds the self-test report of every pack (tests/helpers.ts packsSelfTest) into the run's
// fixtures; tests/run.ts starts it with the shards, so the 13-packs shards only read it.
import { packsSelfTest } from "./helpers";

packsSelfTest();
