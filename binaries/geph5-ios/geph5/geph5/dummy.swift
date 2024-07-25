
import Foundation
import UIKit
import SwiftRs


/// Using ObjC allows us to easily pass Strings / Data
@objc public class GlobalFunctions: NSObject {
    @objc public static func objcDummyFn(data: NSString) {
        
        showAlert(text: data as String)
    }
}


@_cdecl("dummyFn")
func dummyFn() {
    showAlert(text: "I was called via c!")
}


@_cdecl("swiftrs_dummy_fn")
func swiftrsDummyFn(data: SRString) {
    showAlert(text: data.toString())
}


func showAlert(text: String) {
    if let vc = UIApplication.getTopViewController() {
        let alert = UIAlertController(title: "Alert", message: text, preferredStyle: UIAlertController.Style.alert)
        alert.addAction(UIAlertAction(title: "Click", style: UIAlertAction.Style.default, handler: nil))
        vc.present(alert, animated: true, completion: nil)
    }
}


extension UIApplication {

    // Lets us easily get the top-level view controller from anywhere
    class func getTopViewController(base: UIViewController? = UIApplication.shared.keyWindow?.rootViewController) -> UIViewController? {

        if let nav = base as? UINavigationController {
            return getTopViewController(base: nav.visibleViewController)

        } else if let tab = base as? UITabBarController, let selected = tab.selectedViewController {
            return getTopViewController(base: selected)

        } else if let presented = base?.presentedViewController {
            return getTopViewController(base: presented)
        }
        return base
    }
}
