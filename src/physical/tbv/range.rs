// Copyright 2020 The OctoSQL Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::cmp::min;
use std::sync::Arc;

use anyhow::Result;
use arrow::array::{BooleanBuilder, Int64Array, Int64Builder};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;

use crate::physical::expression::Expression;
use crate::physical::physical::{BATCH_SIZE, ExecutionContext, MetaSendFn, Node, ProduceContext, ProduceFn, RETRACTIONS_FIELD, ScalarValue, SchemaContext, NodeMetadata};

pub struct Range {
    start: Arc<dyn Expression>,
    end: Arc<dyn Expression>,
}

impl Range {
    pub fn new(start: Arc<dyn Expression>, end: Arc<dyn Expression>) -> Self {
        Range { start, end }
    }
}

impl Node for Range {
    fn metadata(&self, schema_context: Arc<dyn SchemaContext>) -> Result<NodeMetadata> {
        Ok(NodeMetadata{schema: Arc::new(Schema::new(vec![
            Field::new("i", DataType::Int64, false),
            Field::new(RETRACTIONS_FIELD, DataType::Boolean, false),
        ])), time_field: None })
    }

    // TODO: Put batchsize into execution context.
    fn run(&self, ctx: &ExecutionContext, produce: ProduceFn, meta_send: MetaSendFn) -> Result<()> {
        // Create BATCH_SIZE sized retraction_array for later reuse.
        let mut retraction_array_builder = BooleanBuilder::new(BATCH_SIZE);
        for _i in 0..BATCH_SIZE {
            retraction_array_builder.append_value(false)?;
        }
        let retraction_array = Arc::new(retraction_array_builder.finish());

        let mut start =  if let ScalarValue::Int64(v) = self.start.evaluate_scalar(ctx)? {
            v
        } else {
            Err(anyhow!("range start must be integer"))?
        };
        let end =  if let ScalarValue::Int64(v) = self.end.evaluate_scalar(ctx)? {
            v
        } else {
            Err(anyhow!("range end must be integer"))?
        };

        let output_schema = self.metadata(ctx.variable_context.clone())?.schema;

        while start < end {
            let batch_end = min(start + (BATCH_SIZE as i64), end);

            let mut batch_builder = Int64Builder::new((batch_end - start) as usize);
            for i in start..batch_end {
                batch_builder.append_value(i)?
            }

            let batch_retraction_array = if end - start == (BATCH_SIZE as i64) {
                retraction_array.clone()
            } else {
                let mut retraction_array_builder = BooleanBuilder::new(BATCH_SIZE);
                for i in start..batch_end {
                    retraction_array_builder.append_value(false)?;
                }
                Arc::new(retraction_array_builder.finish())
            };

            let record_batch = RecordBatch::try_new(
                output_schema.clone(),
                vec![
                    Arc::new(batch_builder.finish()),
                    batch_retraction_array,
                ],
            )?;

            produce(&ProduceContext {}, record_batch)?;

            start = batch_end;
        }
        Ok(())
    }
}
