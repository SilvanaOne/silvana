# GetFactoryRequest

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**choice_arguments** | [**serde_json::Value**](.md) | The arguments that are intended to be passed to the choice provided by the factory. To avoid repeating the Daml type definitions, they are specified as JSON objects. However the concrete format is given by how the choice arguments are encoded using the Daml JSON API  The choice arguments are provided so that the registry can also provide choice-argument specific contracts, e.g., the configuration for a specific instrument-id.  | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


