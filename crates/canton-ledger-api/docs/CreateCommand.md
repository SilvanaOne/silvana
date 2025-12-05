# CreateCommand

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**template_id** | **String** | The template of contract the client wants to create. Both package-name and package-id reference identifier formats for the template-id are supported. Note: The package-id reference identifier format is deprecated. We plan to end support for this format in version 3.4.  Required | 
**create_arguments** | Option<[**serde_json::Value**](.md)> | The arguments required for creating a contract from this template. Required | 

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


