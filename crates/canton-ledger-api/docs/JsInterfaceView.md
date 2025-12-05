# JsInterfaceView

## Properties

Name | Type | Description | Notes
------------ | ------------- | ------------- | -------------
**interface_id** | **String** | The interface implemented by the matched event. The identifier uses the package-id reference format.  Required | 
**view_status** | [**models::JsStatus**](JsStatus.md) |  | 
**view_value** | Option<[**serde_json::Value**](.md)> | The value of the interface's view method on this event. Set if it was requested in the ``InterfaceFilter`` and it could be sucessfully computed. Optional | [optional]

[[Back to Model list]](../README.md#documentation-for-models) [[Back to API list]](../README.md#documentation-for-api-endpoints) [[Back to README]](../README.md)


