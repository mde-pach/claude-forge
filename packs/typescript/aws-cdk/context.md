AWS CDK: `RemovalPolicy.DESTROY` and `autoDeleteObjects: true` delete data with the stack (advice); `cdk.context.json` is committed, never git-ignored.
cdk-nag findings are fixed rather than suppressed: a changed file may not add `NagSuppressions.add…Suppressions` calls.
