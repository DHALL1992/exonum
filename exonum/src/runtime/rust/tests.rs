// Copyright 2019 The Exonum Team
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//   http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use exonum_derive::exonum_service;
use exonum_merkledb::{BinaryValue, Database, Entry, Fork, TemporaryDB};

use std::convert::TryFrom;

use crate::{
    proto::{
        schema::tests::{TestServiceInit, TestServiceTx},
        Any,
    },
    runtime::{
        dispatcher::Dispatcher, error::ExecutionError, CallInfo, Caller, ExecutionContext,
        InstanceDescriptor, InstanceId, InstanceSpec,
    },
};

use super::{
    service::{Service, ServiceFactory},
    ArtifactId, Error, RustRuntime, TransactionContext,
};

const SERVICE_INSTANCE_ID: InstanceId = 2;
const SERVICE_INSTANCE_NAME: &str = "test_service_name";

#[derive(Debug, ProtobufConvert)]
#[exonum(pb = "TestServiceInit", crate = "crate")]
struct Init {
    msg: String,
}

#[derive(Debug, ProtobufConvert)]
#[exonum(pb = "TestServiceTx", crate = "crate")]
struct TxA {
    value: u64,
}

#[derive(Debug, ProtobufConvert)]
#[exonum(pb = "TestServiceTx", crate = "crate")]
struct TxB {
    value: u64,
}

#[exonum_service(crate = "crate")]
trait TestService {
    fn method_a(&self, context: TransactionContext, arg: TxA) -> Result<(), ExecutionError>;
    fn method_b(&self, context: TransactionContext, arg: TxB) -> Result<(), ExecutionError>;
}

#[derive(Debug, ServiceFactory)]
#[exonum(
    crate = "crate",
    artifact_name = "test_service",
    artifact_version = "0.1.0",
    proto_sources = "crate::proto::schema",
    service_interface = "TestService"
)]
pub struct TestServiceImpl;

impl TestService for TestServiceImpl {
    fn method_a(&self, mut context: TransactionContext, arg: TxA) -> Result<(), ExecutionError> {
        {
            let fork = context.fork();
            let mut entry = Entry::new("method_a_entry", fork);
            entry.set(arg.value);
        }

        // Test calling one service from another.
        // TODO: It should be improved to support service auth in the future.
        let call_info = CallInfo {
            instance_id: SERVICE_INSTANCE_ID,
            method_id: 1,
        };
        let payload = TxB { value: arg.value }.into_bytes();
        context
            .call(call_info, &payload)
            .expect("Failed to dispatch call");
        Ok(())
    }

    fn method_b(&self, context: TransactionContext, arg: TxB) -> Result<(), ExecutionError> {
        let fork = context.fork();
        let mut entry = Entry::new("method_b_entry", fork);
        entry.set(arg.value);
        Ok(())
    }
}

impl Service for TestServiceImpl {
    fn configure(
        &self,
        _descriptor: InstanceDescriptor,
        fork: &Fork,
        arg: Any,
    ) -> Result<(), ExecutionError> {
        let arg = Init::try_from(arg).map_err(|e| (Error::ConfigParseError, e))?;

        let mut entry = Entry::new("constructor_entry", fork);
        entry.set(arg.msg);
        Ok(())
    }
}

#[test]
fn test_basic_rust_runtime() {
    let db = TemporaryDB::new();

    // Create a runtime and a service.
    let mut runtime = RustRuntime::new();

    let service_factory = Box::new(TestServiceImpl);
    let artifact: ArtifactId = service_factory.artifact_id().into();
    runtime.add_service_factory(service_factory);

    // Create dummy dispatcher.
    let mut dispatcher = Dispatcher::with_runtimes(vec![runtime.into()]);

    // Deploy service.
    let fork = db.fork();
    dispatcher
        .deploy_and_register_artifact(&fork, &artifact, Any::default())
        .unwrap();
    db.merge(fork.into_patch()).unwrap();

    // Init service
    {
        let spec = InstanceSpec {
            artifact,
            id: SERVICE_INSTANCE_ID,
            name: SERVICE_INSTANCE_NAME.to_owned(),
        };

        let constructor = Init {
            msg: "constructor_message".to_owned(),
        }
        .into();

        let fork = db.fork();

        dispatcher.start_service(&fork, spec, constructor).unwrap();
        {
            let entry = Entry::new("constructor_entry", &fork);
            assert_eq!(entry.get(), Some("constructor_message".to_owned()));
        }

        db.merge(fork.into_patch()).unwrap();
    }

    // Execute transaction method A.
    {
        const ARG_A_VALUE: u64 = 11;
        let call_info = CallInfo {
            instance_id: SERVICE_INSTANCE_ID,
            method_id: 0,
        };
        let payload = TxA { value: ARG_A_VALUE }.into_bytes();
        let fork = db.fork();
        let mut context = ExecutionContext::new(&fork, Caller::Blockchain);
        dispatcher.call(&mut context, call_info, &payload).unwrap();

        {
            let entry = Entry::new("method_a_entry", &fork);
            assert_eq!(entry.get(), Some(ARG_A_VALUE));
        }
        {
            let entry = Entry::new("method_b_entry", &fork);
            assert_eq!(entry.get(), Some(ARG_A_VALUE));
        }

        db.merge(fork.into_patch()).unwrap();
    }
    // Execute transaction method B.
    {
        const ARG_B_VALUE: u64 = 22;
        let call_info = CallInfo {
            instance_id: SERVICE_INSTANCE_ID,
            method_id: 1,
        };
        let payload = TxB { value: ARG_B_VALUE }.into_bytes();
        let fork = db.fork();
        let mut context = ExecutionContext::new(&fork, Caller::Blockchain);
        dispatcher.call(&mut context, call_info, &payload).unwrap();

        {
            let entry = Entry::new("method_b_entry", &fork);
            assert_eq!(entry.get(), Some(ARG_B_VALUE));
        }

        db.merge(fork.into_patch()).unwrap();
    }
}
